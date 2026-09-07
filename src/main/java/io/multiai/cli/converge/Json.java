package io.multiai.cli.converge;

import java.util.*;

/**
 * 최소 JSON 파서·직렬화기. SPEC D12 — 외부 Java 라이브러리를 추가하지 않는다.
 *
 * 파싱 대상은 우리가 스키마로 강제한 기계 생성 JSON 뿐이다. 범용 파서가 아니라
 * 그 범위에서 정확히 동작하는 것이 목표다.
 *
 * 매핑: object→LinkedHashMap, array→ArrayList, string→String,
 *       number→Double 또는 Long, true/false→Boolean, null→null
 */
public final class Json {

    public static final class SyntaxException extends RuntimeException {
        public SyntaxException(String m, int pos) {
            super(m + " (위치 " + pos + ")");
        }
    }

    private final String s;
    private int i;

    private Json(String s) {
        this.s = s;
    }

    // ---------- 파싱 ----------

    public static Object parse(String text) {
        Json p = new Json(text);
        p.ws();
        Object v = p.value();
        p.ws();
        if (p.i < p.s.length()) throw new SyntaxException("뒤에 잉여 문자", p.i);
        return v;
    }

    /**
     * 텍스트 안에서 최상위 JSON 객체 하나를 찾아 파싱한다.
     * CLI 가 JSON 앞뒤에 배너나 코드펜스를 붙이는 경우에 쓴다.
     */
    public static Optional<Map<String, Object>> findObject(String text) {
        if (text == null) return Optional.empty();
        int start = text.indexOf('{');
        while (start >= 0) {
            Json p = new Json(text.substring(start));
            try {
                p.ws();
                Object v = p.value();
                if (v instanceof Map<?, ?> m) {
                    @SuppressWarnings("unchecked")
                    Map<String, Object> cast = (Map<String, Object>) m;
                    return Optional.of(cast);
                }
            } catch (RuntimeException ignored) {
                // 이 위치에서 시작하는 JSON 이 아니다. 다음 후보로.
            }
            start = text.indexOf('{', start + 1);
        }
        return Optional.empty();
    }

    private Object value() {
        if (i >= s.length()) throw new SyntaxException("값이 없다", i);
        char c = s.charAt(i);
        return switch (c) {
            case '{' -> object();
            case '[' -> array();
            case '"' -> string();
            case 't' -> literal("true", Boolean.TRUE);
            case 'f' -> literal("false", Boolean.FALSE);
            case 'n' -> literal("null", null);
            default -> number();
        };
    }

    private Map<String, Object> object() {
        Map<String, Object> m = new LinkedHashMap<>();
        expect('{');
        ws();
        if (peek() == '}') { i++; return m; }
        while (true) {
            ws();
            String k = string();
            ws();
            expect(':');
            ws();
            m.put(k, value());
            ws();
            char c = next();
            if (c == '}') return m;
            if (c != ',') throw new SyntaxException("객체에 , 또는 } 가 와야 한다", i - 1);
        }
    }

    private List<Object> array() {
        List<Object> l = new ArrayList<>();
        expect('[');
        ws();
        if (peek() == ']') { i++; return l; }
        while (true) {
            ws();
            l.add(value());
            ws();
            char c = next();
            if (c == ']') return l;
            if (c != ',') throw new SyntaxException("배열에 , 또는 ] 가 와야 한다", i - 1);
        }
    }

    private String string() {
        expect('"');
        StringBuilder sb = new StringBuilder();
        while (true) {
            if (i >= s.length()) throw new SyntaxException("문자열이 닫히지 않았다", i);
            char c = s.charAt(i++);
            if (c == '"') return sb.toString();
            if (c != '\\') { sb.append(c); continue; }
            char e = next();
            switch (e) {
                case '"' -> sb.append('"');
                case '\\' -> sb.append('\\');
                case '/' -> sb.append('/');
                case 'b' -> sb.append('\b');
                case 'f' -> sb.append('\f');
                case 'n' -> sb.append('\n');
                case 'r' -> sb.append('\r');
                case 't' -> sb.append('\t');
                case 'u' -> {
                    if (i + 4 > s.length()) throw new SyntaxException("\\u 이스케이프가 짧다", i);
                    sb.append((char) Integer.parseInt(s.substring(i, i + 4), 16));
                    i += 4;
                }
                default -> throw new SyntaxException("알 수 없는 이스케이프 \\" + e, i - 1);
            }
        }
    }

    private Object number() {
        int st = i;
        if (peek() == '-') i++;
        while (i < s.length() && (Character.isDigit(s.charAt(i)) || "+-.eE".indexOf(s.charAt(i)) >= 0)) i++;
        String t = s.substring(st, i);
        if (t.isEmpty()) throw new SyntaxException("숫자가 아니다", st);
        try {
            return (t.contains(".") || t.contains("e") || t.contains("E"))
                    ? (Object) Double.valueOf(t) : (Object) Long.valueOf(t);
        } catch (NumberFormatException e) {
            throw new SyntaxException("숫자 형식 오류: " + t, st);
        }
    }

    private Object literal(String word, Object v) {
        if (!s.startsWith(word, i)) throw new SyntaxException("리터럴이 아니다", i);
        i += word.length();
        return v;
    }

    private void ws() {
        while (i < s.length() && Character.isWhitespace(s.charAt(i))) i++;
    }

    private char peek() {
        return i < s.length() ? s.charAt(i) : '\0';
    }

    private char next() {
        if (i >= s.length()) throw new SyntaxException("입력이 끝났다", i);
        return s.charAt(i++);
    }

    private void expect(char c) {
        if (next() != c) throw new SyntaxException("'" + c + "' 가 와야 한다", i - 1);
    }

    // ---------- 직렬화 ----------

    /** 스키마 파일을 쓸 때만 쓴다. 문자열 이스케이프가 핵심이다. */
    public static String escape(String s) {
        StringBuilder sb = new StringBuilder(s.length() + 16);
        for (int k = 0; k < s.length(); k++) {
            char c = s.charAt(k);
            switch (c) {
                case '"' -> sb.append("\\\"");
                case '\\' -> sb.append("\\\\");
                case '\n' -> sb.append("\\n");
                case '\r' -> sb.append("\\r");
                case '\t' -> sb.append("\\t");
                case '\b' -> sb.append("\\b");
                case '\f' -> sb.append("\\f");
                default -> {
                    if (c < 0x20) sb.append(String.format("\\u%04x", (int) c));
                    else sb.append(c);
                }
            }
        }
        return sb.toString();
    }

    // ---------- 접근 헬퍼 ----------

    public static String str(Map<String, Object> m, String key, String def) {
        Object v = m.get(key);
        return v instanceof String s ? s : def;
    }

    @SuppressWarnings("unchecked")
    public static List<Object> list(Map<String, Object> m, String key) {
        Object v = m.get(key);
        return v instanceof List<?> l ? (List<Object>) l : List.of();
    }

    @SuppressWarnings("unchecked")
    public static Map<String, Object> map(Object o) {
        return o instanceof Map<?, ?> m ? (Map<String, Object>) m : Map.of();
    }
}
