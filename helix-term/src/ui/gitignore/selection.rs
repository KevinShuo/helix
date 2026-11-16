use crate::gitignore::{Template, TEMPLATES};
use std::collections::BTreeSet;

pub(super) struct TemplateSelection {
    pub selected: BTreeSet<&'static str>,
    order: Vec<usize>,
    pub filtered: Vec<usize>,
    pub cursor: usize,
}

impl TemplateSelection {
    pub fn new(detected: &[&str]) -> Self {
        let selected = TEMPLATES
            .iter()
            .filter(|t| detected.contains(&t.id))
            .map(|t| t.id)
            .collect::<BTreeSet<_>>();
        let mut order = (0..TEMPLATES.len()).collect::<Vec<_>>();
        order.sort_by_key(|&i| !selected.contains(TEMPLATES[i].id));
        Self {
            selected,
            filtered: order.clone(),
            order,
            cursor: 0,
        }
    }
    pub fn filter(&mut self, query: &str) {
        let query = query.to_lowercase();
        let terms = query.split_whitespace().collect::<Vec<_>>();
        self.filtered = self
            .order
            .iter()
            .copied()
            .filter(|&i| {
                let t = &TEMPLATES[i];
                let haystack =
                    format!("{} {} {} {}", t.id, t.name, t.category, t.aliases).to_lowercase();
                terms.iter().all(|term| haystack.contains(term))
            })
            .collect();
        self.cursor = 0;
    }
    pub fn current(&self) -> Option<&'static Template> {
        self.filtered.get(self.cursor).map(|&i| &TEMPLATES[i])
    }
    pub fn toggle(&mut self) {
        if let Some(template) = self.current() {
            if !self.selected.remove(template.id) {
                self.selected.insert(template.id);
            }
        }
    }
    pub fn move_by(&mut self, backwards: bool) {
        let len = self.filtered.len();
        if len > 0 {
            self.cursor = (self.cursor + if backwards { len - 1 } else { 1 }) % len;
        }
    }
}
