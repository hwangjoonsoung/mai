package io.multiai.cli.ui;

import io.multiai.cli.provider.ProviderResult;

import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.util.*;

/**
 * 화면 분할 UI.
 *
 * 답변을 순서대로 쏟으면 읽기 어렵다는 실사용 피드백에 따라, 참여자마다 **자기 칸**을
 * 갖는 전체 화면 레이아웃으로 바꿨다.
 *
 * 질문 전:                        질문 후:
 *   ┌──────────────────┐            ┌──────┬────────┬────────┬────────┐
 *   │   multi_ai_cli   │            │ dir  │ claude │ codex  │  agy   │
 *   │  ┌────────────┐  │            ├──────┤        │        │        │
 *   │  │ 질문 입력  │  │            │ session      │        │        │
 *   │  └────────────┘  │            └──────┴────────┴────────┴────────┘
 *   └──────────────────┘
 *
 * 외부 라이브러리 없이 ANSI 이스케이프만 쓴다 (D12). 단일 키 입력을 받을 수 없으므로
 * 조작은 줄 단위 명령(/v, /scroll)으로 한다.
 */
public final class TuiRenderer {

    /** 참여자 칸 하나의 상태. */
    public static final class Pane {
        public final String id;
        public final String title;
        public String status = "대기";
        public String body = "";
        public String elapsed = "";
        public int scroll = 0;

        public Pane(String id, String title) {
            this.id = id;
            this.title = title;
        }

        public void from(ProviderResult r) {
            this.status = switch (r.status()) {
                case OK, UNPARSED -> "완료";
                case FAILED -> "실패";
                case TIMEOUT -> "타임아웃";
                case CANCELLED -> "취소";
                case UNAVAILABLE -> "사용불가";
            };
            this.elapsed = String.format("%.1fs", r.elapsed().toMillis() / 1000.0);
            this.body = r.text().isBlank()
                    ? (r.failureNote().isBlank() ? "(응답 없음)" : "! " + r.failureNote())
                    : r.text().strip();
        }
    }

    private static final int LEFT_W = 22;   // dir / session 칸 폭

    private final PrintStream out;
    private final List<Pane> panes = new ArrayList<>();
    private String workspace = "";
    private String roomLine = "";
    private String sessionInfo = "";
    private String question = "";
    private String hint = "";
    private boolean started = false;

    public TuiRenderer() {
        this.out = new PrintStream(new java.io.FileOutputStream(java.io.FileDescriptor.out),
                true, StandardCharsets.UTF_8);
    }

    // ---------- 상태 갱신 ----------

    public void setProviders(List<String> ids, List<String> titles) {
        panes.clear();
        for (int i = 0; i < ids.size(); i++) panes.add(new Pane(ids.get(i), titles.get(i)));
    }

    public void setWorkspace(String s) { this.workspace = s; }
    public void setSession(String room, String info) { this.roomLine = room; this.sessionInfo = info; }
    public void setHint(String s) { this.hint = s; }

    public void startRound(String question, List<String> targetIds) {
        this.question = question;
        this.started = true;
        for (Pane p : panes) {
            boolean on = targetIds.contains(p.id);
            p.status = on ? "실행 중" : "제외";
            p.body = on ? "" : "";
            p.elapsed = "";
            p.scroll = 0;
        }
    }

    public Optional<Pane> pane(String key) {
        for (int i = 0; i < panes.size(); i++) {
            Pane p = panes.get(i);
            if (p.id.equalsIgnoreCase(key) || String.valueOf(i + 1).equals(key)) {
                return Optional.of(p);
            }
        }
        return Optional.empty();
    }

    public void update(ProviderResult r) {
        pane(r.providerId()).ifPresent(p -> p.from(r));
    }

    public List<Pane> panes() { return panes; }

    // ---------- 그리기 ----------

    /** 전체 화면을 다시 그린다. 상태가 바뀔 때마다 호출한다. */
    public void draw() {
        int cols = Term.cols();
        int rows = Term.rows();
        StringBuilder b = new StringBuilder(cols * rows + 512);
        b.append(Term.clearScreen()).append(Term.hideCursor());

        if (!started) {
            drawIdle(b, cols, rows);
        } else {
            drawPanes(b, cols, rows);
        }
        b.append(Term.showCursor());
        out.print(b);
        out.flush();
    }

    /** 질문 전 화면 — 제목과 입력 안내만. */
    private void drawIdle(StringBuilder b, int cols, int rows) {
        int boxW = Math.min(64, cols - 8);
        int top = Math.max(3, rows / 2 - 5);
        int left = (cols - boxW) / 2 + 1;

        b.append(Term.moveTo(top, 1)).append(Term.center("multi_ai_cli", cols));
        b.append(Term.moveTo(top + 1, 1))
         .append(Term.DIM).append(Term.center(titleLine(), cols)).append(Term.RESET);

        b.append(Term.moveTo(top + 4, left)).append("┌").append("─".repeat(boxW - 2)).append("┐");
        b.append(Term.moveTo(top + 5, left)).append("│")
         .append(Term.center("질문을 입력하세요", boxW - 2)).append("│");
        b.append(Term.moveTo(top + 6, left)).append("└").append("─".repeat(boxW - 2)).append("┘");

        b.append(Term.moveTo(top + 8, 1)).append(Term.DIM)
         .append(Term.center("/help 명령 목록   ·   /exit 종료", cols)).append(Term.RESET);
    }

    private String titleLine() {
        StringBuilder s = new StringBuilder();
        for (Pane p : panes) {
            if (s.length() > 0) s.append("  ·  ");
            s.append(p.title);
        }
        return s.toString();
    }

    /** 질문 후 화면 — 왼쪽에 dir/session, 오른쪽에 참여자별 칸. */
    private void drawPanes(StringBuilder b, int cols, int rows) {
        int inner = cols - 2;
        int n = Math.max(1, panes.size());
        int paneW = (inner - LEFT_W - 1) / n;
        int bodyTop = 4;
        int bodyH = rows - bodyTop - 3;

        // 상단: 질문
        b.append(Term.moveTo(1, 1)).append("┌").append("─".repeat(inner)).append("┐");
        b.append(Term.moveTo(2, 1)).append("│ ").append(Term.BOLD)
         .append(Term.pad(Term.cut(question, inner - 2), inner - 2)).append(Term.RESET).append("│");
        b.append(Term.moveTo(3, 1)).append("├").append("─".repeat(inner)).append("┤");

        // 왼쪽 dir / session
        int dirH = Math.max(3, bodyH / 2);
        box(b, bodyTop, 2, LEFT_W, dirH, "dir",
                Term.wrap(workspace, LEFT_W - 4));
        box(b, bodyTop + dirH, 2, LEFT_W, bodyH - dirH, "session",
                Term.wrap(roomLine + "\n" + sessionInfo, LEFT_W - 4));

        // 참여자 칸
        for (int i = 0; i < panes.size(); i++) {
            Pane p = panes.get(i);
            int x = 2 + LEFT_W + 1 + i * paneW;
            String head = p.title + (p.elapsed.isEmpty() ? "" : " " + p.elapsed);
            List<String> lines = Term.wrap(p.body.isEmpty() ? "(" + p.status + ")" : p.body, paneW - 4);
            box(b, bodyTop, x, paneW, bodyH, head + "  [" + p.status + "]", slice(lines, p, bodyH - 2));
        }

        // 하단 안내
        b.append(Term.moveTo(rows - 1, 1)).append(Term.DIM)
         .append(Term.pad(" " + (hint.isEmpty() ? defaultHint() : hint), cols)).append(Term.RESET);
        b.append(Term.moveTo(rows, 1));
    }

    private String defaultHint() {
        return "/v <번호|이름> 한 칸 크게 · /s <번호> <줄> 스크롤 · /n 새 질문 · /exit";
    }

    /** 칸 높이에 맞춰 잘라내고, 남은 줄이 있으면 알린다. */
    private List<String> slice(List<String> lines, Pane p, int h) {
        if (lines.size() <= h) return lines;
        int from = Math.max(0, Math.min(p.scroll, lines.size() - h));
        List<String> out = new ArrayList<>(lines.subList(from, Math.min(lines.size(), from + h - 1)));
        out.add("… " + (lines.size() - from - h + 1) + "줄 더  (/v " + p.id + ")");
        return out;
    }

    /** 제목이 달린 상자 하나를 그린다. */
    private void box(StringBuilder b, int top, int left, int w, int h, String title, List<String> body) {
        if (w < 6 || h < 3) return;
        b.append(Term.moveTo(top, left)).append("┌ ")
         .append(Term.cut(title, w - 4)).append(" ")
         .append("─".repeat(Math.max(0, w - 4 - Term.width(Term.cut(title, w - 4)))))
         .append("┐");
        for (int i = 1; i < h - 1; i++) {
            String line = i - 1 < body.size() ? body.get(i - 1) : "";
            b.append(Term.moveTo(top + i, left)).append("│ ")
             .append(Term.pad(line, w - 4)).append(" │");
        }
        b.append(Term.moveTo(top + h - 1, left)).append("└").append("─".repeat(w - 2)).append("┘");
    }

    /** 한 참여자의 답변만 전체 화면으로. 긴 답을 읽을 때 쓴다. */
    public void drawSingle(Pane p) {
        int cols = Term.cols();
        int rows = Term.rows();
        StringBuilder b = new StringBuilder();
        b.append(Term.clearScreen()).append(Term.hideCursor());
        b.append(Term.moveTo(1, 1)).append("┌").append("─".repeat(cols - 2)).append("┐");
        b.append(Term.moveTo(2, 1)).append("│ ").append(Term.BOLD)
         .append(Term.pad(p.title + "  [" + p.status + "]  " + p.elapsed, cols - 4))
         .append(Term.RESET).append(" │");
        b.append(Term.moveTo(3, 1)).append("├").append("─".repeat(cols - 2)).append("┤");

        List<String> lines = Term.wrap(p.body, cols - 4);
        int h = rows - 6;
        int from = Math.max(0, Math.min(p.scroll, Math.max(0, lines.size() - h)));
        for (int i = 0; i < h; i++) {
            String line = from + i < lines.size() ? lines.get(from + i) : "";
            b.append(Term.moveTo(4 + i, 1)).append("│ ").append(Term.pad(line, cols - 4)).append(" │");
        }
        b.append(Term.moveTo(rows - 2, 1)).append("└").append("─".repeat(cols - 2)).append("┘");
        b.append(Term.moveTo(rows - 1, 1)).append(Term.DIM)
         .append(Term.pad(" " + (from + h < lines.size()
                        ? "/s " + p.id + " " + (from + h) + " 다음 · "
                        : "") + "/v 로 전체 화면 복귀", cols))
         .append(Term.RESET);
        b.append(Term.showCursor()).append(Term.moveTo(rows, 1));
        out.print(b);
        out.flush();
    }

    // ---------- 입력 줄 ----------

    public void prompt() {
        out.print("> ");
        out.flush();
    }

    /** 화면을 지우지 않고 하단에 한 줄만 알린다. */
    public void status(String msg) {
        out.print(Term.moveTo(Term.rows() - 1, 1) + Term.pad(" " + msg, Term.cols())
                + Term.moveTo(Term.rows(), 1));
        out.flush();
    }

    public void plain(String s) {
        out.println(s);
    }

    /** 종료 시 화면을 정리하고 커서를 되살린다. 안 하면 터미널이 망가진 채 남는다. */
    public void restore() {
        out.print(Term.showCursor() + Term.RESET + Term.clearScreen());
        out.flush();
    }
}
