import pytest
from collections import Counter

from subject import unique_by


def test_preserves_first_item_per_key_in_order():
    '''普通场景：重复键保留首次元素，结果顺序与首次出现顺序一致。'''
    items = [
        {'id': 1, 'name': 'first'},
        {'id': 2, 'name': 'second'},
        {'id': 1, 'name': 'third'},
        {'id': 3, 'name': 'fourth'},
        {'id': 2, 'name': 'fifth'},
    ]
    result = unique_by(items, lambda item: item['id'])
    assert result == [items[0], items[1], items[3]]
    assert result[0] is items[0]


def test_key_called_once_per_item_including_duplicates():
    '''副作用场景：key 对每个元素恰好调用一次，包括重复元素。'''
    items = ['alpha', 'beta', 'alpha', 'gamma', 'beta']
    calls = []

    def key(item):
        calls.append(item)
        return len(item)

    result = unique_by(items, key)
    assert result == ['alpha', 'beta']
    assert len(calls) == len(items)
    assert Counter(calls) == Counter(items)


def test_all_same_key_keeps_only_first_item():
    '''边界场景：所有元素键相同时只保留第一个元素。'''
    items = ['a', 'b', 'c']
    result = unique_by(items, lambda _: 'same')
    assert result == ['a']
    assert result[0] is items[0]


def test_empty_input_returns_list():
    '''边界场景：空输入返回空 list。'''
    result = unique_by([], lambda item: item)
    assert result == []
    assert isinstance(result, list)


def test_accepts_non_list_iterable_and_returns_list():
    '''边界场景：非 list 可迭代输入仍返回 list。'''
    items = (1, 2, 1, 3)
    result = unique_by(items, lambda item: item % 2)
    assert result == [1, 2]
    assert isinstance(result, list)


def test_does_not_mutate_input_list():
    '''普通场景：不原地修改传入的 list。'''
    items = ['x', 'y', 'x', 'z']
    original = list(items)
    result = unique_by(items, lambda item: item)
    assert result == ['x', 'y', 'z']
    assert items == original


def test_key_exception_propagates():
    '''错误场景：key 抛出的异常会向外传播。'''
    class ExpectedError(Exception):
        pass

    def key(item):
        if item == 'bad':
            raise ExpectedError('key failed')
        return item

    with pytest.raises(ExpectedError, match='key failed'):
        unique_by(['ok', 'bad'], key)
