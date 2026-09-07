package io.multiai.cli.process;

import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.time.Duration;
import java.util.*;
import java.util.concurrent.*;

/**
 * OS 공통 프로세스 기동·취소. SPEC §6.3 · §6.4.
 *
 * OS 별로 다른 것은 **강제 종료 폴백 명령 하나뿐**이다. 스트림 소비, stdin 주입,
 * 타임아웃, 트리 종료 절차는 전부 공통이다. 그래서 macOS 이식이 하위 클래스
 * 하나 추가로 끝난다 (§6.4 완료 기준).
 *
 * - 인수는 문자열 배열로만 전달한다. 셸을 경유하지 않는다.
 * - stdout·stderr 를 별도 스레드에서 동시에 소비해 버퍼 교착을 막는다.
 * - stdin 은 프롬프트 주입 후, 주입이 없으면 즉시 close 한다.
 * - 입출력 디코딩은 UTF-8 고정.
 */
public abstract class AbstractProcessLauncher implements ProcessLauncher {

    private final Path fallbackDir;
    private final ExecutorService pumps = Executors.newCachedThreadPool(r -> {
        Thread t = new Thread(r, "stream-pump");
        t.setDaemon(true);
        return t;
    });
    /** label -> 실행 중인 프로세스. /cancel 대상 조회용. */
    private final Map<String, Process> live = new ConcurrentHashMap<>();
    /**
     * 아직 기동되지 않은 참여자에 대한 취소 요청.
     *
     * 취소가 프로세스 등록보다 먼저 도착할 수 있다 — 라운드 시작 직후에 /cancel 이
     * 들어오면 live 맵이 비어 있어 조용히 무시된다. 그 경쟁 조건을 막는다.
     */
    private final Set<String> pendingCancel = ConcurrentHashMap.newKeySet();

    /** @param fallbackDir invocation 이 runDir 을 주지 않을 때 쓸 위치 */
    protected AbstractProcessLauncher(Path fallbackDir) {
        this.fallbackDir = fallbackDir;
    }

    /**
     * OS 기본 유틸리티로 프로세스 트리를 강제 종료한다.
     * 외부 Java 라이브러리가 아니므로 D12 를 위반하지 않는다.
     *
     * @return 성공 여부. false 면 호출부가 destroyForcibly() 로 넘어간다.
     */
    protected abstract boolean forceKillTree(long pid);

    /** 자식에게 물려주지 않을 환경변수 (SPEC §9.2 Q5 — Claude 중첩 실행 대응). */
    protected boolean isSessionMarker(String key) {
        String k = key.toUpperCase(Locale.ROOT);
        return k.equals("CLAUDECODE") || k.startsWith("CLAUDE_CODE_") || k.equals("CLAUDE_SESSION_ID");
    }

    @Override
    public RunOutcome run(Invocation inv) throws InterruptedException {
        long t0 = System.nanoTime();
        Path dir = inv.runDir() != null ? inv.runDir() : fallbackDir;
        try {
            Files.createDirectories(dir);
        } catch (IOException e) {
            dir = fallbackDir;
        }
        Path outF = dir.resolve(inv.label() + ".stdout.txt");
        Path errF = dir.resolve(inv.label() + ".stderr.txt");

        ProcessBuilder pb = new ProcessBuilder(inv.command());
        pb.directory(inv.workspace().toFile());
        pb.environment().keySet().removeIf(this::isSessionMarker);

        Process p;
        try {
            p = pb.start();
        } catch (IOException e) {
            return new RunOutcome(RunOutcome.Status.LAUNCH_ERROR, -1, "", "",
                    null, null, elapsed(t0), "기동 실패: " + e.getMessage());
        }
        live.put(inv.label(), p);
        if (pendingCancel.remove(inv.label())) {
            // 기동 전에 도착한 취소를 여기서 소진한다.
            List<Long> survivors = terminateTree(p);
            live.remove(inv.label(), p);
            return new RunOutcome(RunOutcome.Status.CANCELLED, -1, "", "",
                    null, null, elapsed(t0), "취소됨" + survivorNote(survivors));
        }
        try {
            writeStdin(p, inv.stdinBody());
            Path fo1 = outF, fe1 = errF;
            Future<String> fo = pumps.submit(() -> drain(p.getInputStream(), fo1));
            Future<String> fe = pumps.submit(() -> drain(p.getErrorStream(), fe1));

            boolean done = p.waitFor(inv.timeout().toMillis(), TimeUnit.MILLISECONDS);
            if (!done) {
                List<Long> survivors = terminateTree(p);
                return new RunOutcome(RunOutcome.Status.TIMEOUT, -1,
                        get(fo), get(fe), outF, errF, elapsed(t0),
                        "타임아웃 " + inv.timeout().toSeconds() + "초" + survivorNote(survivors));
            }
            String out = get(fo), err = get(fe);
            int code = p.exitValue();
            RunOutcome.Status st = code == 0 ? RunOutcome.Status.OK : RunOutcome.Status.FAILED;
            return new RunOutcome(st, code, out, err, outF, errF, elapsed(t0),
                    code == 0 ? "" : "종료 코드 " + code);
        } finally {
            live.remove(inv.label(), p);
        }
    }

    /** SPEC §7.2 — 프롬프트를 stdin 으로 주입하고 즉시 닫는다. 주입이 없어도 닫는다. */
    private void writeStdin(Process p, String body) {
        try (OutputStream os = p.getOutputStream()) {
            if (body != null && !body.isEmpty()) {
                os.write(body.getBytes(StandardCharsets.UTF_8));
                os.flush();
            }
        } catch (IOException ignored) {
            // 자식이 stdin 을 읽지 않고 먼저 끝나면 파이프가 닫혀 있을 수 있다.
        }
    }

    /** 스트림을 끝까지 읽으면서 원본 파일에도 남긴다. */
    private String drain(InputStream in, Path file) {
        StringBuilder sb = new StringBuilder();
        try (BufferedReader r = new BufferedReader(new InputStreamReader(in, StandardCharsets.UTF_8));
             BufferedWriter w = Files.newBufferedWriter(file, StandardCharsets.UTF_8)) {
            String line;
            while ((line = r.readLine()) != null) {
                sb.append(line).append('\n');
                w.write(line);
                w.write('\n');
            }
        } catch (IOException ignored) {
            // 부분 수집이라도 보존한다.
        }
        return sb.toString();
    }

    @Override
    public void cancel(String label) {
        Process p = live.get(label);
        if (p != null) {
            terminateTree(p);
        } else {
            // 아직 안 떴다. 뜨는 즉시 죽이도록 표시해둔다.
            pendingCancel.add(label);
        }
    }

    @Override
    public void shutdown() {
        live.values().forEach(this::terminateTree);
        pumps.shutdownNow();
    }

    /**
     * SPEC §6.3 프로세스 트리 종료 절차. 보장 수준은 best-effort 다.
     * 부모를 루프 안에서 죽이지 않는다 — 죽이면 descendants() 의 기준이 사라지고
     * OS 트리 종료 명령도 쓸 수 없다 (교차검토 R5·C3).
     *
     * @return 종료를 확인하지 못한 생존 PID. 관측·추적한 핸들에 한정된다.
     */
    protected List<Long> terminateTree(Process parent) {
        ProcessHandle ph = parent.toHandle();
        for (int attempt = 0; attempt < 3; attempt++) {
            List<ProcessHandle> kids = ph.descendants().toList();
            if (kids.isEmpty()) break;
            // 깊은 것부터. descendants() 는 대체로 얕은 순이라 뒤집어서 리프를 먼저 친다.
            for (int i = kids.size() - 1; i >= 0; i--) kids.get(i).destroyForcibly();
            awaitQuiet(kids);
            if (ph.descendants().findAny().isEmpty()) break;
        }
        if (ph.isAlive()) {
            if (!forceKillTree(ph.pid())) parent.destroyForcibly();
            awaitQuiet(List.of(ph));
        }
        List<Long> survivors = new ArrayList<>();
        if (ph.isAlive()) survivors.add(ph.pid());
        ph.descendants().filter(ProcessHandle::isAlive).map(ProcessHandle::pid).forEach(survivors::add);
        return survivors;
    }

    /** OS 유틸리티를 짧게 실행하고 성공 여부만 돌려준다. */
    protected boolean runQuiet(List<String> cmd) {
        try {
            Process k = new ProcessBuilder(cmd).redirectErrorStream(true).start();
            k.getOutputStream().close();
            k.getInputStream().readAllBytes();
            return k.waitFor(5, TimeUnit.SECONDS) && k.exitValue() == 0;
        } catch (IOException e) {
            return false;
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return false;
        }
    }

    private void awaitQuiet(List<ProcessHandle> hs) {
        try {
            CompletableFuture<?>[] fs = hs.stream().map(ProcessHandle::onExit)
                    .toArray(CompletableFuture[]::new);
            CompletableFuture.allOf(fs).get(2, TimeUnit.SECONDS);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        } catch (ExecutionException | TimeoutException ignored) {
            // 유예를 넘겨도 다음 단계에서 처리한다.
        }
    }

    private static String survivorNote(List<Long> pids) {
        if (pids.isEmpty()) return " (추적 범위에서 생존 프로세스 없음)";
        return " — 생존 PID " + pids + " (추적한 범위 기준)";
    }

    private static String get(Future<String> f) {
        try {
            return f.get(5, TimeUnit.SECONDS);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return "";
        } catch (ExecutionException | TimeoutException e) {
            return "";
        }
    }

    private static Duration elapsed(long t0) {
        return Duration.ofNanos(System.nanoTime() - t0);
    }
}
