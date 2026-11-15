import pytest
from subject import unique_by

def test_contract():
    values = ['a', 'A', 'b', 'B', 'c']
    before = list(values)
    calls = []
    def key(value):
        calls.append(value)
        return value.lower()
    assert unique_by(values, key) == ['a', 'b', 'c']
    assert values == before
    assert calls == values
    assert unique_by([], key) == []
    def broken(value):
        raise LookupError('sentinel')
    with pytest.raises(LookupError):
        unique_by([1], broken)
