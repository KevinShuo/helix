from subject import parse_port
import pytest


def test_正常端口号():
    """基本数字字符串应正确解析为整数"""
    assert parse_port("80") == 80


def test_首尾空白被去除():
    """前后空格、制表符、换行应被 strip 后正常解析"""
    assert parse_port("  443  ") == 443
    assert parse_port("\t8080\n") == 8080


def test_前导零接受():
    """前导零不影响数值解析"""
    assert parse_port("007") == 7
    assert parse_port("00001") == 1


def test_最小有效端口():
    """端口 1 为下边界，应合法"""
    assert parse_port("1") == 1


def test_最大有效端口():
    """端口 65535 为上边界，应合法"""
    assert parse_port("65535") == 65535


def test_空字符串抛异常():
    """空字符串应抛出 ValueError"""
    with pytest.raises(ValueError):
        parse_port("")


def test_纯空白抛异常():
    """仅含空白字符，strip 后为空，应抛出 ValueError"""
    with pytest.raises(ValueError):
        parse_port("   \t\n  ")


def test_正号前缀抛异常():
    """带正号的字符串非纯十进制数字，应抛出 ValueError"""
    with pytest.raises(ValueError):
        parse_port("+80")


def test_负号前缀抛异常():
    """带负号的字符串非纯十进制数字，应抛出 ValueError"""
    with pytest.raises(ValueError):
        parse_port("-80")


def test_非ASCII全角数字抛异常():
    """全角数字非 ASCII，应抛出 ValueError"""
    with pytest.raises(ValueError):
        parse_port("８０")


def test_含字母抛异常():
    """数字中混入字母，非纯十进制，应抛出 ValueError"""
    with pytest.raises(ValueError):
        parse_port("8a")


def test_中间含空格抛异常():
    """strip 后内部仍有空格，非纯十进制，应抛出 ValueError"""
    with pytest.raises(ValueError):
        parse_port("8 0")


def test_零值抛异常():
    """端口 0 低于下界 1，应抛出 ValueError"""
    with pytest.raises(ValueError):
        parse_port("0")


def test_全零前导抛异常():
    """全零字符串解析为 0，低于下界，应抛出 ValueError"""
    with pytest.raises(ValueError):
        parse_port("00000")


def test_超出上限抛异常():
    """端口 65536 超过上界，应抛出 ValueError"""
    with pytest.raises(ValueError):
        parse_port("65536")


def test_非ASCII拉丁字符抛异常():
    """含非 ASCII 拉丁字符，应抛出 ValueError"""
    with pytest.raises(ValueError):
        parse_port("é80")
