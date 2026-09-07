package io.multiai.cli.orchestration;

import io.multiai.cli.provider.*;
import io.multiai.cli.room.*;

import java.nio.file.Path;
import java.time.Duration;
import java.util.*;
import java.util.concurrent.*;

/**
 * 한 라운드의 병렬 실행. SPEC §7.4.
 *
 * - 대상 참여자에게 **같은 프롬프트**를 동시에 보낸다.
 * - 한 AI 의 타임아웃·비정상 종료가 다른 AI 를 취소하지 않는다 (§4.3-6 부분 실패 허용).
 * - 완료 순서대로 콜백을 호출해 발화자 블록을 즉시 출력한다.
 */
public final class ParallelRoundExecutor implements AutoCloseable {

    private final ExecutorService pool = Executors.newCachedThreadPool(r -> {
        Thread t = new Thread(r, "round-worker");
        t.setDaemon(true);
        return t;
    });

    /** 실행 결과가 완료되는 대로 호출된다. */
    public interface Sink {
        void onResult(ProviderResult r);
    }

    /**
     * @param targets 이번 라운드 대상 참여자
     * @param prompt  전 참여자 공통 프롬프트 (§7.2 전송 규약)
     * @return 완료 순서대로 담긴 결과
     */
    public List<ProviderResult> run(List<AiProvider> targets, String prompt, Path workspace,
                                    boolean write, Path runDir, Path tempDir,
                                    Duration timeout, Sink sink) {
        return run(targets, prompt, workspace, write, runDir, tempDir, timeout, null, sink);
    }

    /** @param schemaFile 구조화 출력 스키마. null 이면 평문 text 로 받는다 (Phase 3). */
    public List<ProviderResult> run(List<AiProvider> targets, String prompt, Path workspace,
                                    boolean write, Path runDir, Path tempDir,
                                    Duration timeout, Path schemaFile, Sink sink) {

        CompletionService<ProviderResult> cs = new ExecutorCompletionService<>(pool);
        for (AiProvider p : targets) {
            cs.submit(() -> {
                try {
                    return p.invoke(prompt, workspace, write, runDir, tempDir, timeout, schemaFile);
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    return cancelled(p);
                } catch (RuntimeException e) {
                    // 어댑터 내부 오류를 AI 응답으로 가장하지 않는다 (§7.7).
                    return new ProviderResult(p.id(), p.displayName(),
                            ProviderResult.Status.FAILED, "", -1, Duration.ZERO,
                            null, null, "어댑터 오류: " + e);
                }
            });
        }

        List<ProviderResult> out = new ArrayList<>(targets.size());
        for (int i = 0; i < targets.size(); i++) {
            try {
                ProviderResult r = cs.take().get();
                out.add(r);
                sink.onResult(r);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                break;
            } catch (ExecutionException e) {
                // submit 한 작업은 예외를 삼키므로 여기 도달하지 않는 것이 정상이다.
            }
        }
        return out;
    }

    private static ProviderResult cancelled(AiProvider p) {
        return new ProviderResult(p.id(), p.displayName(), ProviderResult.Status.CANCELLED,
                "", -1, Duration.ZERO, null, null, "취소됨");
    }

    @Override
    public void close() {
        pool.shutdownNow();
    }
}
