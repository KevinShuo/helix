import pytest
from subject import parse_port

def test_contract():
    assert parse_port('1') == 1
    assert parse_port('65535') == 65535
    assert parse_port(' 00080\t') == 80
    for value in ['', '+1', '-1', '0', '65536', '１２', 'abc']:
        with pytest.raises(ValueError):
            parse_port(value)
