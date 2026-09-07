package io.multiai.cli.ui;

import io.multiai.cli.provider.ProviderResult;

import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.List;

/**
 * 콘솔 출력. SPEC §5.1 · 요구사항 4.
 *
 * 세 답변을 한꺼번에 쏟으면 읽을 수가 없다. 그래서 라운드가 끝나면 **요약표만**
 * 보여주고, 사용자가 `/v <번호>` 로 원하는 답변 구역만 펼쳐 보게 한다.
 * 한 참여자의 블록 안에 다른 참여자의 출력이 섞이지 않는다는 요구는 그대로다.
 *
 * 출력 스트림은 UTF-8 고정. Windows 콘솔 코드페이지는 run.ps1 에서 chcp 65001 로
 * 맞춘다 — 스트림 인코딩과 콘솔 코드페이지는 별개다 (§6.3, 교차검토 R11).
 */
public final class ConsoleRenderer {

    private static final String RULE = "─".repeat(60);
    private static final String THICK = "━".repeat(60);

    private final PrintStream out;

    public ConsoleRenderer() {
        this.out = new PrintStream(new java.io.FileOutputStream(java.io.FileDescriptor.out),
                true, StandardCharsets.UTF_8);
    }

    public void print(String s) { out.println(s); }
    public void blank() { out.println(); }

    public void prompt(String roomName) {
        out.print("multi-ai(" + roomName + ")> ");
        out.flush();
    }

    /** 라운드 시작. 대상 참여자를 실행 중으로 표시한다. */
    public void running(List<String> displayNames) {
        out.println();
        out.println("  " + String.join(" · ", displayNames) + "  실행 중...");
    }

    /** 한 참여자가 끝날 때마다 한 줄. 진행 상황만 알리고 본문은 안 쏟는다. */
    public void tick(int index, ProviderResult r) {
        out.printf("  [%d] %s  %s  %s  %s%n",
                index, padDisplay(r.displayName(), 16), badge(r), secs(r.elapsed()), size(r));
    }

    /** 라운드 종료 요약. 여기서 무엇을 펼쳐 볼지 고르게 한다. */
    public void roundSummary(List<ProviderResult> results) {
        out.println();
        out.println("  " + RULE);
        for (int i = 0; i < results.size(); i++) {
            ProviderResult r = results.get(i);
            out.printf("  [%d] %s  %s  %s  %s%n",
                    i + 1, padDisplay(r.displayName(), 16), badge(r), secs(r.elapsed()), size(r));
            if (!r.ok() && !r.failureNote().isBlank()) {
                out.println("      " + r.failureNote());
                if (r.stderrFile() != null) out.println("      stderr: " + r.stderrFile());
            }
        }
        out.println("  " + RULE);
        out.println("  /v <번호>  답변 보기   ·   /v all  전체   ·   /v  마지막 요약 다시");
        out.println();
    }

    /**
     * 한 참여자의 답변 구역. 위아래를 굵은 선으로 막아 다른 답변과 섞이지 않게 한다.
     */
    public void answer(int index, ProviderResult r) {
        out.println();
        out.println(THICK);
        out.printf("  [%d] %s   %s · %s · %s%n",
                index, r.displayName(), badge(r), secs(r.elapsed()), size(r));
        out.println(THICK);
        out.println();
        if (r.text().isBlank()) {
            out.println("  (응답 없음)");
        } else {
            out.println(r.text().stripTrailing());
        }
        if (!r.ok()) {
            // 프로세스 오류를 AI 응답으로 가장하지 않는다 (SPEC §7.7).
            out.println();
            out.println("  ! 실행 실패: " + r.failureNote());
            if (r.stderrFile() != null) out.println("  ! stderr: " + r.stderrFile());
        }
        out.println();
        out.println(THICK);
        out.println();
    }

    public void error(String msg) {
        out.println("  ! " + msg);
    }

    public void notice(String msg) {
        out.println("  · " + msg);
    }

    private static String badge(ProviderResult r) {
        return switch (r.status()) {
            case OK -> "성공";
            case UNPARSED -> "성공(원문)";
            case FAILED -> "실패";
            case TIMEOUT -> "타임아웃";
            case CANCELLED -> "취소됨";
            case UNAVAILABLE -> "사용불가";
        };
    }

    private static String size(ProviderResult r) {
        int n = r.text() == null ? 0 : r.text().strip().length();
        return n == 0 ? "—" : String.format("%,d자", n);
    }

    private static String secs(Duration d) {
        return String.format("%5.1fs", d.toMillis() / 1000.0);
    }

    /** 한글은 두 칸을 차지한다. 표가 어긋나지 않게 표시 폭으로 채운다. */
    private static String padDisplay(String s, int width) {
        int w = 0;
        for (int i = 0; i < s.length(); i++) w += charWidth(s.charAt(i));
        return w >= width ? s : s + " ".repeat(width - w);
    }

    private static int charWidth(char c) {
        // CJK 및 한글 영역은 폭 2로 센다.
        return (c >= 0x1100 && c <= 0x115F)
                || (c >= 0x2E80 && c <= 0xA4CF)
                || (c >= 0xAC00 && c <= 0xD7A3)
                || (c >= 0xF900 && c <= 0xFAFF)
                || (c >= 0xFE30 && c <= 0xFE6F)
                || (c >= 0xFF00 && c <= 0xFF60)
                || (c >= 0xFFE0 && c <= 0xFFE6) ? 2 : 1;
    }
}
