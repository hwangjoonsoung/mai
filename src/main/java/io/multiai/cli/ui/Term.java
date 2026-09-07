package io.multiai.cli.ui;

import java.util.ArrayList;
import java.util.List;

/**
 * 터미널 제어 원시 기능. SPEC D12 — 외부 라이브러리 없이 ANSI 이스케이프만 쓴다.
 *
 * 크기는 JVM 이 직접 알 수 없으므로 실행 스크립트가 -Dterm.cols/-Dterm.rows 로
 * 넘겨준다. 값이 없으면 보수적인 기본값을 쓴다.
 */
public final class Term {

    public static final String ESC = "\u001B[";
    public static final String RESET = ESC + "0m";
    public static final String DIM = ESC + "2m";
    public static final String BOLD = ESC + "1m";

    private Term() {}

    public static int cols() {
        return clamp(intProp("term.cols", 120), 60, 400);
    }

    public static int rows() {
        return clamp(intProp("term.rows", 30), 16, 200);
    }

    private static int intProp(String key, int def) {
        try {
            return Integer.parseInt(System.getProperty(key, String.valueOf(def)).trim());
        } catch (NumberFormatException e) {
            return def;
        }
    }

    private static int clamp(int v, int lo, int hi) {
        return Math.max(lo, Math.min(hi, v));
    }

    public static String clearScreen() { return ESC + "2J" + ESC + "H"; }
    public static String moveTo(int row, int col) { return ESC + row + ";" + col + "H"; }
    public static String hideCursor() { return ESC + "?25l"; }
    public static String showCursor() { return ESC + "?25h"; }

    // ---------- 폭 계산 ----------

    /** 한글·CJK 는 두 칸을 차지한다. 박스가 어긋나지 않으려면 이걸로 세야 한다. */
    public static int width(char c) {
        return (c >= 0x1100 && c <= 0x115F)
                || (c >= 0x2E80 && c <= 0xA4CF)
                || (c >= 0xAC00 && c <= 0xD7A3)
                || (c >= 0xF900 && c <= 0xFAFF)
                || (c >= 0xFE30 && c <= 0xFE6F)
                || (c >= 0xFF00 && c <= 0xFF60)
                || (c >= 0xFFE0 && c <= 0xFFE6) ? 2 : 1;
    }

    public static int width(String s) {
        int w = 0;
        for (int i = 0; i < s.length(); i++) w += width(s.charAt(i));
        return w;
    }

    /** 표시 폭 기준으로 자른다. 두 칸 문자가 경계에 걸리면 통째로 뺀다. */
    public static String cut(String s, int max) {
        int w = 0;
        StringBuilder b = new StringBuilder();
        for (int i = 0; i < s.length(); i++) {
            int cw = width(s.charAt(i));
            if (w + cw > max) break;
            b.append(s.charAt(i));
            w += cw;
        }
        return b.toString();
    }

    /** 표시 폭 기준 좌측 정렬 패딩. */
    public static String pad(String s, int width) {
        String t = cut(s, width);
        return t + " ".repeat(Math.max(0, width - width(t)));
    }

    public static String center(String s, int width) {
        String t = cut(s, width);
        int left = Math.max(0, (width - width(t)) / 2);
        return " ".repeat(left) + t + " ".repeat(Math.max(0, width - left - width(t)));
    }

    /**
     * 본문을 지정 폭으로 접는다. 원문 개행은 보존하고, 긴 줄만 나눈다.
     * 단어 경계를 우선하되 한 단어가 폭을 넘으면 강제로 자른다.
     */
    public static List<String> wrap(String text, int width) {
        List<String> out = new ArrayList<>();
        if (text == null) return out;
        for (String raw : text.split("\n", -1)) {
            String line = raw.replace("\t", "    ");
            if (line.isEmpty()) { out.add(""); continue; }
            while (width(line) > width) {
                int cutAt = line.length();
                int w = 0;
                for (int i = 0; i < line.length(); i++) {
                    w += width(line.charAt(i));
                    if (w > width) { cutAt = i; break; }
                }
                // 공백이 있으면 거기서 끊어 단어를 보존한다.
                int sp = line.lastIndexOf(' ', cutAt);
                int end = (sp > width / 3) ? sp : cutAt;
                out.add(line.substring(0, end));
                line = line.substring(sp > width / 3 ? end + 1 : end);
            }
            out.add(line);
        }
        return out;
    }
}
