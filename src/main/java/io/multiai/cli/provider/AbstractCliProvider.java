package io.multiai.cli.provider;

import io.multiai.cli.process.*;

import java.nio.file.Path;
import java.time.Duration;
import java.util.List;

/**
 * CLI 어댑터 공통부. 프로세스 실행과 결과 정규화의 뼈대만 갖는다.
 * 공급자별 옵션 차이는 각 서브클래스 안에서만 처리한다 (SPEC §4.3-2).
 */
abstract class AbstractCliProvider implements AiProvider {

    protected final ResolvedCommand command;
    protected final ProcessLauncher launcher;

    protected AbstractCliProvider(ResolvedCommand command, ProcessLauncher launcher) {
        this.command = command;
        this.launcher = launcher;
    }

    @Override
    public ResolvedCommand command() {
        return command;
    }

    /** 공급자별 인수 목록. launcher 선행 인수는 호출부에서 붙인다. */
    protected abstract List<String> arguments(String prompt, Path workspace, boolean write,
                                              Path tempDir, Duration timeout, Path schemaFile);

    /** 원시 출력에서 사용자에게 보일 최종 텍스트를 뽑는다. */
    protected abstract String extractText(RunOutcome outcome, Path tempDir);

    @Override
    public ProviderResult invoke(String prompt, Path workspace, boolean write,
                                 Path runDir, Path tempDir, Duration timeout, Path schemaFile)
            throws InterruptedException {

        List<String> argv = new java.util.ArrayList<>(command.launcher());
        argv.addAll(arguments(prompt, workspace, write, tempDir, timeout, schemaFile));

        Invocation inv = new Invocation(
                argv, workspace, acceptsStdin() ? prompt : null, timeout, id(), runDir);
        RunOutcome out = launcher.run(inv);

        String text = out.ok() ? extractText(out, tempDir) : "";
        ProviderResult.Status st = switch (out.status()) {
            case OK -> text.isBlank() ? ProviderResult.Status.UNPARSED : ProviderResult.Status.OK;
            case TIMEOUT -> ProviderResult.Status.TIMEOUT;
            case CANCELLED -> ProviderResult.Status.CANCELLED;
            case LAUNCH_ERROR -> ProviderResult.Status.UNAVAILABLE;
            case FAILED -> ProviderResult.Status.FAILED;
        };
        // 파싱 실패 시 원문을 그대로 보여준다 (SPEC §7.7).
        if (st == ProviderResult.Status.UNPARSED) text = out.stdout();

        return new ProviderResult(id(), displayName(), st, text, out.exitCode(),
                out.elapsed(), out.stdoutFile(), out.stderrFile(), out.failureNote());
    }
}
