package io.multiai.cli.process;

/**
 * 프로세스 기동과 취소. SPEC §6.4 — OS 의존 실행 로직의 경계.
 */
public interface ProcessLauncher {

    /** 프로세스를 기동하고 종료까지 기다린 결과를 돌려준다. */
    RunOutcome run(Invocation invocation) throws InterruptedException;

    /** 실행 중인 프로세스를 취소한다. SPEC §6.3 프로세스 트리 종료 절차. */
    void cancel(String label);

    /** 종료 훅에서 남은 자식 프로세스를 정리한다. */
    void shutdown();
}
