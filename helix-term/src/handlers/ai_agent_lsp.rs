//! Read-only semantic lookups using the servers already attached to open buffers.
use futures_util::{stream::FuturesUnordered, StreamExt};
use helix_core::{chars::char_is_word, syntax::config::LanguageServerFeature, Rope, Uri};
use helix_lsp::{lsp, util::pos_to_lsp_pos, Client};
use helix_stdx::path::normalize;
use helix_view::{DocumentId, Editor};
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

#[derive(Clone)]
struct Buffer {
    text: Rope,
    identifier: lsp::TextDocumentIdentifier,
    definitions: Vec<Arc<Client>>,
    references: Vec<Arc<Client>>,
}

#[derive(Clone, Default)]
pub(super) struct Semantic {
    root: PathBuf,
    buffers: HashMap<PathBuf, Buffer>,
    versions: HashMap<PathBuf, (DocumentId, i32)>,
}

impl Semantic {
    pub(super) fn capture(editor: &Editor, root: &Path) -> Self {
        let root = normalize(root);
        let mut buffers = HashMap::new();
        let mut versions = HashMap::new();
        for doc in editor.documents() {
            let Some(path) = doc.path().map(normalize).filter(|p| p.starts_with(&root)) else {
                continue;
            };
            versions.insert(path.clone(), (doc.id(), doc.version()));
            let servers = |feature| {
                doc.language_servers_with_feature(feature)
                    .filter_map(|ls| editor.language_servers.get_by_id(ls.id()).cloned())
                    .collect::<Vec<_>>()
            };
            let definitions = servers(LanguageServerFeature::GotoDefinition);
            let references = servers(LanguageServerFeature::GotoReference);
            if definitions.is_empty() && references.is_empty() {
                continue;
            }
            if let Some(url) = doc.url() {
                buffers.insert(
                    path,
                    Buffer {
                        text: doc.text().clone(),
                        identifier: lsp::TextDocumentIdentifier::new(url),
                        definitions,
                        references,
                    },
                );
            }
        }
        Self {
            root,
            buffers,
            versions,
        }
    }

    /// The workspace tool checks both the origin and returned paths against its
    /// file inventory before reading or exposing a result.
    pub(super) async fn lookup(
        &self,
        path: &Path,
        line: usize,
        column: Option<usize>,
        symbol: &str,
        references: bool,
    ) -> Vec<(PathBuf, usize)> {
        let origin = normalize(path);
        let Some(doc) = self.buffers.get(&origin) else {
            return vec![];
        };
        let Some(offset) = symbol_offset(&doc.text, line, column, symbol) else {
            return vec![];
        };
        let clients = if references {
            &doc.references
        } else {
            &doc.definitions
        };
        let mut queries: FuturesUnordered<_> = clients
            .iter()
            .map(|client| {
                let position = pos_to_lsp_pos(&doc.text, offset, client.offset_encoding());
                let position =
                    lsp::TextDocumentPositionParams::new(doc.identifier.clone(), position);
                query(client, position, references)
            })
            .collect();
        let mut results = BTreeSet::new();
        // Dropping outstanding queries on timeout or user cancellation sends
        // $/cancelRequest. Retain results from servers that already replied.
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            while let Some(locations) = queries.next().await {
                for location in locations {
                    let Ok(uri) = Uri::try_from(location.uri) else {
                        continue;
                    };
                    let Some(path) = uri.as_path().filter(|p| p.starts_with(&self.root)) else {
                        continue;
                    };
                    let Some(line) = (location.range.start.line as usize).checked_add(1) else {
                        continue;
                    };
                    results.insert((path.to_owned(), line));
                    if results.len() >= 64 {
                        return;
                    }
                }
            }
        })
        .await;
        if results.is_empty() {
            return vec![];
        }
        // The server reads live buffers. If an edit arrived while the model
        // was choosing a tool, its locations may no longer describe our ropes.
        let source_version = self.versions.get(&origin).copied();
        let results: Vec<_> = results
            .into_iter()
            .map(|(path, line)| {
                let expected = self.versions.get(&path).copied();
                (path, line, expected)
            })
            .collect();
        let (send, receive) = tokio::sync::oneshot::channel();
        tokio::time::timeout(Duration::from_secs(2), async move {
            crate::job::dispatch(move |editor, _| {
                let matches = |path: &Path, expected: Option<(DocumentId, i32)>| {
                    if let Some((id, version)) = expected {
                        editor.document(id).is_some_and(|doc| {
                            doc.version() == version
                                && doc.path().is_some_and(|p| normalize(p) == path)
                        })
                    } else {
                        editor
                            .document_by_path(path)
                            .is_none_or(|doc| !doc.is_modified())
                    }
                };
                let filtered = if matches(&origin, source_version) {
                    results
                        .into_iter()
                        .filter(|(p, _, version)| matches(p, *version))
                        .map(|(p, line, _)| (p, line))
                        .collect()
                } else {
                    Vec::new()
                };
                let _ = send.send(filtered);
            })
            .await;
            receive.await.unwrap_or_default()
        })
        .await
        .unwrap_or_default()
    }
}

fn symbol_offset(text: &Rope, line: usize, column: Option<usize>, symbol: &str) -> Option<usize> {
    let ident = |c: char| char_is_word(c) || c == '$';
    if symbol.is_empty() || !symbol.chars().all(ident) {
        return None;
    }
    let row = line.checked_sub(1)?;
    let line = text.get_line(row)?;
    if line.len_bytes() > 64 * 1024 {
        return None;
    }
    let line = line.to_string();
    let mut matches = line.match_indices(symbol).filter(|(start, matched)| {
        !line[..*start].chars().next_back().is_some_and(ident)
            && !line[*start + matched.len()..]
                .chars()
                .next()
                .is_some_and(ident)
    });
    let start = if let Some(col) = column {
        let byte = line.char_indices().nth(col.checked_sub(1)?)?.0;
        matches
            .find(|(start, s)| (*start..*start + s.len()).contains(&byte))?
            .0
    } else {
        let first = matches.next()?.0;
        if matches.next().is_some() {
            return None;
        }
        first
    };
    Some(text.line_to_char(row) + line[..start].chars().count())
}

async fn query(
    client: &Client,
    at: lsp::TextDocumentPositionParams,
    references: bool,
) -> Vec<lsp::Location> {
    if references {
        return client
            .call_cancellable::<lsp::request::References>(lsp::ReferenceParams {
                text_document_position: at,
                context: lsp::ReferenceContext {
                    include_declaration: false,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
    }
    let response = client
        .call_cancellable::<lsp::request::GotoDefinition>(lsp::GotoDefinitionParams {
            text_document_position_params: at,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .ok()
        .flatten();
    match response {
        Some(lsp::GotoDefinitionResponse::Scalar(location)) => vec![location],
        Some(lsp::GotoDefinitionResponse::Array(locations)) => locations,
        Some(lsp::GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| lsp::Location::new(link.target_uri, link.target_selection_range))
            .collect(),
        None => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_unicode_columns_and_refuses_ambiguous_tokens() {
        let text = Rope::from("marker\n😀 更长项 + 项 + 项\n");
        let first = "marker\n😀 更长项 + ".chars().count();
        assert_eq!(symbol_offset(&text, 2, None, "项"), None);
        assert_eq!(symbol_offset(&text, 2, Some(9), "项"), Some(first));
        assert_eq!(symbol_offset(&text, 2, Some(13), "项"), Some(first + 4));
        assert_eq!(symbol_offset(&text, 2, Some(5), "项"), None);
        assert_eq!(symbol_offset(&text, 1, None, "marker"), Some(0));
        assert_eq!(symbol_offset(&text, 0, None, "marker"), None);
    }
}
