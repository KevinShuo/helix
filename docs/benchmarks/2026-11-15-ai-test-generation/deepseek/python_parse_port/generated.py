import pytest

from subject import parse_port


def test_parse_port_普通十进制端口():
    """普通十进制端口应返回对应整数"""
    assert parse_port("80") == 80
    assert parse_port("443") == 443
    assert parse_port("8080") == 8080


def test_parse_port_去除首尾空白():
    """首尾空白应被去除后解析"""
    assert parse_port("  80  ") == 80
    assert parse_port("\t443\n") == 443
    assert parse_port("\r\n8080\t ") == 8080


def test_parse_port_接受前导零():
    """前导零应被接受"""
    assert parse_port("0080") == 80
    assert parse_port("00000001") == 1
    assert parse_port("065535") == 65535


def test_parse_port_边界值():
    """1 与 65535 为合法边界"""
    assert parse_port("1") == 1
    assert parse_port("65535") == 65535


def test_parse_port_超出范围():
    """0 与 65536 超出范围应抛 ValueError"""
    with pytest.raises(ValueError):
        parse_port("0")
    with pytest.raises(ValueError):
        parse_port("65536")
    with pytest.raises(ValueError):
        parse_port("999999")


def test_parse_port_空字符串():
    """空字符串应抛 ValueError"""
    with pytest.raises(ValueError):
        parse_port("")


def test_parse_port_仅空白():
    """仅含空白字符应抛 ValueError"""
    with pytest.raises(ValueError):
        parse_port("   ")
    with pytest.raises(ValueError):
        parse_port("\t\n")


def test_parse_port_带符号():
    """带正负号应抛 ValueError"""
    with pytest.raises(ValueError):
        parse_port("+80")
    with pytest.raises(ValueError):
        parse_port("-80")


def test_parse_port_非十进制字符():
    """含非十进制字符应抛 ValueError"""
    with pytest.raises(ValueError):
        parse_port("8a")
    with pytest.raises(ValueError):
        parse_port("80.0")
    with pytest.raises(ValueError):
        parse_port("1 2")


def test_parse_port_非ASCII数字():
    """非 ASCII 数字字符应抛 ValueError"""
    with pytest.raises(ValueError):
        parse_port("８０")
    with pytest.raises(ValueError):
        parse_port("١٢٣")
