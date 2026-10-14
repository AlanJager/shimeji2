package shimeji2.ref;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * JSON subset used by scenario files: objects, arrays, strings, integers, booleans, and null.
 * Numbers must be integers. They are returned as {@link Long}.
 */
public final class Json {
    private Json() {
    }

    public static Object parse(String text) {
        Parser parser = new Parser(text);
        Object value = parser.parseValue();
        parser.skip();
        if (!parser.done()) {
            parser.fail("trailing data");
        }
        return value;
    }

    private static final class Parser {
        private final String text;
        private int index;

        Parser(String text) {
            this.text = text;
        }

        boolean done() {
            return index >= text.length();
        }

        Object parseValue() {
            skip();
            if (done()) {
                fail("unexpected end");
            }
            char c = text.charAt(index);
            return switch (c) {
                case '{' -> parseObject();
                case '[' -> parseArray();
                case '"' -> parseString();
                case 't' -> parseLiteral("true", Boolean.TRUE);
                case 'f' -> parseLiteral("false", Boolean.FALSE);
                case 'n' -> parseLiteral("null", null);
                default -> parseNumber();
            };
        }

        private Map<String, Object> parseObject() {
            expect('{');
            Map<String, Object> object = new LinkedHashMap<>();
            skip();
            if (peek('}')) {
                index++;
                return object;
            }
            while (true) {
                skip();
                if (done() || text.charAt(index) != '"') {
                    fail("object key must be a string");
                }
                String key = parseString();
                skip();
                expect(':');
                object.put(key, parseValue());
                skip();
                if (peek('}')) {
                    index++;
                    return object;
                }
                expect(',');
            }
        }

        private List<Object> parseArray() {
            expect('[');
            List<Object> array = new ArrayList<>();
            skip();
            if (peek(']')) {
                index++;
                return array;
            }
            while (true) {
                array.add(parseValue());
                skip();
                if (peek(']')) {
                    index++;
                    return array;
                }
                expect(',');
            }
        }

        private String parseString() {
            expect('"');
            StringBuilder out = new StringBuilder();
            while (!done()) {
                char c = text.charAt(index++);
                if (c == '"') {
                    return out.toString();
                }
                if (c == '\\') {
                    if (done()) {
                        fail("truncated escape");
                    }
                    char esc = text.charAt(index++);
                    switch (esc) {
                        case '"', '\\', '/' -> out.append(esc);
                        case 'b' -> out.append('\b');
                        case 'f' -> out.append('\f');
                        case 'n' -> out.append('\n');
                        case 'r' -> out.append('\r');
                        case 't' -> out.append('\t');
                        case 'u' -> {
                            if (index + 4 > text.length()) {
                                fail("truncated unicode escape");
                            }
                            int code = Integer.parseInt(text.substring(index, index + 4), 16);
                            out.append((char) code);
                            index += 4;
                        }
                        default -> fail("bad escape \\" + esc);
                    }
                } else if (c < 0x20) {
                    fail("raw control character in string");
                } else {
                    out.append(c);
                }
            }
            fail("unterminated string");
            return "";
        }

        private Object parseLiteral(String literal, Object value) {
            if (!text.startsWith(literal, index)) {
                fail("expected " + literal);
            }
            index += literal.length();
            return value;
        }

        private Long parseNumber() {
            int start = index;
            if (peek('-')) {
                index++;
            }
            if (done() || !isDigit(text.charAt(index))) {
                fail("expected integer");
            }
            if (text.charAt(index) == '0') {
                index++;
            } else {
                while (!done() && isDigit(text.charAt(index))) {
                    index++;
                }
            }
            if (!done()) {
                char c = text.charAt(index);
                if (c == '.' || c == 'e' || c == 'E') {
                    fail("numbers must be integers");
                }
            }
            try {
                return Long.parseLong(text.substring(start, index));
            } catch (NumberFormatException e) {
                fail("integer out of range");
                return 0L;
            }
        }

        private void skip() {
            while (!done()) {
                char c = text.charAt(index);
                if (c == ' ' || c == '\n' || c == '\r' || c == '\t') {
                    index++;
                } else {
                    return;
                }
            }
        }

        private boolean peek(char c) {
            return !done() && text.charAt(index) == c;
        }

        private void expect(char c) {
            if (!peek(c)) {
                fail("expected '" + c + "'");
            }
            index++;
        }

        private static boolean isDigit(char c) {
            return c >= '0' && c <= '9';
        }

        private void fail(String message) {
            throw new IllegalArgumentException(message + " at index " + index);
        }
    }
}
