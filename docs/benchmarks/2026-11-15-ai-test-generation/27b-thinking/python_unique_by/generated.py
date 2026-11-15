import pytest
from subject import unique_by


def test_basic_dedup_preserves_order():
    """基本去重：保留每个 key 首次出现的元素，顺序不变。"""
    items = [1, 2, 1, 3, 2]
    result = unique_by(items, key=lambda x: x)
    assert result == [1, 2, 3]


def test_all_unique_returns_all():
    """所有 key 均不同时，返回全部元素。"""
    items = ["a", "b", "c"]
    result = unique_by(items, key=lambda x: x)
    assert result == ["a", "b", "c"]


def test_all_same_key_returns_first_only():
    """所有元素 key 相同时，仅返回第一个。"""
    items = [10, 20, 30]
    result = unique_by(items, key=lambda x: 0)
    assert result == [10]


def test_empty_items_returns_empty_list():
    """空输入返回空列表。"""
    result = unique_by([], key=lambda x: x)
    assert result == []
    assert isinstance(result, list)


def test_single_item():
    """单个元素直接返回包含该元素的列表。"""
    result = unique_by([42], key=lambda x: x)
    assert result == [42]


def test_key_called_exactly_once_per_item():
    """key 对每个元素（含重复）恰好调用一次。"""
    call_count = 0

    def counting_key(x):
        nonlocal call_count
        call_count += 1
        return x % 2

    items = [1, 3, 2, 4, 5]
    unique_by(items, key=counting_key)
    assert call_count == 5


def test_does_not_mutate_items():
    """调用后原始 items 不被修改。"""
    items = [1, 2, 1, 3]
    original = list(items)
    unique_by(items, key=lambda x: x)
    assert items == original


def test_key_exception_propagates():
    """key 抛出异常时向上传播。"""
    def bad_key(x):
        raise ValueError("boom")

    with pytest.raises(ValueError, match="boom"):
        unique_by([1, 2, 3], key=bad_key)


def test_returns_list_even_for_tuple_input():
    """输入为元组时，返回值仍为 list 类型。"""
    result = unique_by((1, 2, 1, 3), key=lambda x: x)
    assert isinstance(result, list)
    assert result == [1, 2, 3]


def test_non_identity_key_dedup():
    """使用非恒等 key 进行去重（如按奇偶分组）。"""
    items = [1, 3, 2, 4, 5]
    result = unique_by(items, key=lambda x: x % 2)
    assert result == [1, 2]


def test_first_occurrence_is_kept_not_last():
    """重复 key 保留的是首次出现的元素而非最后一次。"""
    items = ["x1", "x2", "x3"]
    result = unique_by(items, key=lambda x: x[0])
    assert result == ["x1"]


def test_key_called_for_duplicates_too():
    """重复 key 的元素也会触发 key 调用（验证调用次数）。"""
    calls = []

    def tracking_key(x):
        calls.append(x)
        return x

    items = ["a", "a", "b"]
    unique_by(items, key=tracking_key)
    assert calls == ["a", "a", "b"]
