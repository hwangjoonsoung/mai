package io.multiai.cli.provider;

import io.multiai.cli.process.*;

import java.nio.file.Path;
import java.time.Duration;

/**
 * 공급자 공통 계약. SPEC §7.3.
 * 세 참여자는 모두 이 계약을 따르며, 역할은 제품이 아니라 프롬프트가 정한다(§7.5).
 */
public interface AiProvider {

    /** 라우팅·저장에 쓰는 안정적 식별자. claude / codex / gemini */
    String id();

    /** 화면 표시명. 예: "Gemini via agy" */
    String displayName();

    /** 멘션 토큰. 예: @gemini */
    default String mention() {
        return "@" + id();
    }

    /** 이 공급자가 프롬프트를 stdin 으로 받는지. false 면 인수로 전달한다(§5.3 실측). */
    boolean acceptsStdin();

    /** 해석된 실행 경로. /status 표시와 가용성 판정에 쓴다. */
    ResolvedCommand command();

    /**
     * 한 라운드를 실행한다.
     *
     * @param prompt    조립된 프롬프트 (전 공급자 동일 내용)
     * @param workspace 대상 워크스페이스 (D18)
     * @param write     쓰기 프로필 사용 여부. Phase 1 에서는 항상 false
     * @param runDir    원본 stdout·stderr 를 남길 디렉터리
     * @param tempDir   워크스페이스 밖 임시 출력 디렉터리 (§7.6)
     * @param schemaFile 구조화 출력 스키마 파일. null 이면 평문 text 로 받는다.
     *                   Windows 에서 인라인 JSON 인자는 깨지므로 반드시 경로다 (§1.5).
     */
    ProviderResult invoke(String prompt, Path workspace, boolean write,
                          Path runDir, Path tempDir, Duration timeout, Path schemaFile)
            throws InterruptedException;
}
