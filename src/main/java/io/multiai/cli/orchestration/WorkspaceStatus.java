package io.multiai.cli.orchestration;

import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.TimeUnit;

/**
 * 쓰기 실행 후 변경 파일 수집. SPEC §7.5 「쓰기 실행」 3·4항.
 *
 * - Git 저장소면 `git status --short`, SVN 저장소면 `svn status` 를 읽기 전용으로 실행한다.
 * - **커밋·푸시·SVN 커밋은 절대 수행하지 않는다.** 조회만 한다.
 */
public final class WorkspaceStatus {

    public enum Vcs { GIT, SVN, NONE }

    public record Report(Vcs vcs, List<String> lines, String note) {
        public boolean hasChanges() {
            return !lines.isEmpty();
        }
    }

    private WorkspaceStatus() {}

    public static Report collect(Path workspace) {
        Vcs vcs = detect(workspace);
        return switch (vcs) {
            case GIT -> run(workspace, Vcs.GIT, List.of("git", "status", "--short"));
            case SVN -> run(workspace, Vcs.SVN, List.of("svn", "status"));
            case NONE -> new Report(Vcs.NONE, List.of(), "버전관리 저장소가 아니다 — 변경 목록을 수집하지 않았다");
        };
    }

    private static Vcs detect(Path ws) {
        for (Path p = ws; p != null; p = p.getParent()) {
            if (Files.isDirectory(p.resolve(".git"))) return Vcs.GIT;
            if (Files.isDirectory(p.resolve(".svn"))) return Vcs.SVN;
        }
        return Vcs.NONE;
    }

    private static Report run(Path ws, Vcs vcs, List<String> cmd) {
        try {
            Process p = new ProcessBuilder(cmd)
                    .directory(ws.toFile())
                    .redirectErrorStream(true)
                    .start();
            p.getOutputStream().close();
            List<String> lines = new ArrayList<>();
            try (BufferedReader r = new BufferedReader(
                    new InputStreamReader(p.getInputStream(), StandardCharsets.UTF_8))) {
                String line;
                while ((line = r.readLine()) != null) {
                    if (!line.isBlank()) lines.add(line);
                }
            }
            if (!p.waitFor(30, TimeUnit.SECONDS)) {
                p.destroyForcibly();
                return new Report(vcs, lines, "상태 조회 타임아웃");
            }
            return new Report(vcs, lines, p.exitValue() == 0 ? "" : "종료 코드 " + p.exitValue());
        } catch (IOException e) {
            return new Report(vcs, List.of(), vcs + " 명령을 실행하지 못했다: " + e.getMessage());
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return new Report(vcs, List.of(), "중단됨");
        }
    }
}
