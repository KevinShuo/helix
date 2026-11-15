'''parse_port 的单元测试。'''

import pytest

from subject import parse_port


def test_strip_whitespace_and_parse():
    '''去除首尾空白后按十进制解析。'''
    assert parse_port(' 8080 ') == 8080
    assert parse_port('\t443\n') == 443
    assert parse_port('  80') == 80
    assert parse_port('80  ') == 80


def test_accept_leading_zeros():
    '''接受前导零。'''
    assert parse_port('0080') == 80
    assert parse_port('0000000000008080') == 8080


def test_port_range_boundaries():
    '''端口合法范围边界 1 与 65535。'''
    assert parse_port('1') == 1
    assert parse_port('65535') == 65535


@pytest.mark.parametrize('text', ['', '   ', '\t\n', '\n  \t'])
def test_empty_or_whitespace_raises(text):
    '''空字符串或仅空白字符应抛出 ValueError。'''
    with pytest.raises(ValueError):
        parse_port(text)


@pytest.mark.parametrize('text', ['0', '0000', '65536', '99999'])
def test_out_of_range_raises(text):
    '''超出 1..65535 范围应抛出 ValueError。'''
    with pytest.raises(ValueError):
        parse_port(text)


@pytest.mark.parametrize('text', ['+80', '-80', '  -1 ', '+0', '-0'])
def test_signed_numbers_raise(text):
    '''带正负号的数字应抛出 ValueError。'''
    with pytest.raises(ValueError):
        parse_port(text)


@pytest.mark.parametrize(
    'text',
    ['80.5', '80a', '0x50', '8 0', '8,0', '８０', '٨٠', '١٢٣'],
)
def test_non_ascii_or_non_decimal_raises(text):
    '''非 ASCII 数字或非十进制形式应抛出 ValueError。'''
    with pytest.raises(ValueError):
        parse_port(text)
