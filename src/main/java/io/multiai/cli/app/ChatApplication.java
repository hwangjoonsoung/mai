package io.multiai.cli.app;

import io.multiai.cli.orchestration.ParallelRoundExecutor;
import io.multiai.cli.orchestration.PromptPresetRepository;
import io.multiai.cli.orchestration.WorkspaceStatus;
import io.multiai.cli.converge.*;
import io.multiai.cli.process.*;
import io.multiai.cli.provider.*;
import io.multiai.cli.room.*;
import io.multiai.cli.ui.ConsoleRenderer;
import io.multiai.cli.ui.TuiRenderer;

import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.time.Duration;
import java.util.*;

/**
 * 채팅방 루프. SPEC §5 · §7.4.
 *
 * 사용자 입력 → 명령/멘션 파싱 → 대상 결정 → 공통 문맥 생성 → 대상별 프롬프트
 * → 병렬 실행 → 결과 정규화 → 완료 순서대로 출력 → 라운드 결과를 방에 저장.
 */
public final class ChatApplication implements AutoCloseable {

    /** SPEC D16 — Java 가 전 공급자 공통 600초를 강제한다. */
    private static final Duration TIMEOUT = Duration.ofSeconds(600);

    private final RoomRepository repo;
    private final List<AiProvider> providers;
    private final ProcessLauncher launcher;
    private final ConsoleRenderer ui;
    /** 화면 분할 UI. null 이면 평문 모드로 떨어진다. */
    private final TuiRenderer tui;
    private final CommandParser parser;
    private final ParallelRoundExecutor executor = new ParallelRoundExecutor();
    private final PromptPresetRepository presets;

    private ChatRoom room;
    private BufferedReader input;
    /** 직전 라운드 결과. /v 로 구역별 열람할 때 쓴다. */
    private List<ProviderResult> lastResults = List.of();
    /** 라운드 실행 중 들어온 입력. 버리지 않고 라운드가 끝난 뒤 처리한다. */
    private final Deque<String> pending = new ArrayDeque<>();

    public ChatApplication(RoomRepository repo, List<AiProvider> providers,
                           ProcessLauncher launcher, ConsoleRenderer ui, ChatRoom room) {
        this(repo, providers, launcher, ui, room, null);
    }

    public ChatApplication(RoomRepository repo, List<AiProvider> providers,
                           ProcessLauncher launcher, ConsoleRenderer ui, ChatRoom room,
                           TuiRenderer tui) {
        this.tui = tui;
        this.repo = repo;
        this.providers = providers;
        this.launcher = launcher;
        this.ui = ui;
        this.room = room;
        this.parser = new CommandParser(providerIds());
        this.presets = new PromptPresetRepository(repo.home().resolve("presets.properties"));
        try {
            presets.load();
        } catch (IOException e) {
            ui.error("프리셋을 읽지 못했다: " + e.getMessage());
        }
    }

    private Set<String> providerIds() {
        Set<String> ids = new LinkedHashSet<>();
        for (AiProvider p : providers) ids.add(p.id());
        return ids;
    }

    public void run() throws IOException {
        if (tui != null) {
            tui.setProviders(providers.stream().map(AiProvider::id).toList(),
                    providers.stream().map(AiProvider::displayName).toList());
            refreshSideInfo();
            tui.draw();
        } else {
            banner();
        }
        try (BufferedReader in = new BufferedReader(
                new InputStreamReader(System.in, StandardCharsets.UTF_8))) {
            this.input = in;
            while (true) {
                // 라운드 중 미리 타이핑해둔 입력을 먼저 소화한다.
                String line;
                if (!pending.isEmpty()) {
                    line = pending.pollFirst();
                    if (tui != null) tui.prompt(); else ui.prompt(room.name());
                    ui.print(line);
                } else {
                    if (tui != null) tui.prompt(); else ui.prompt(room.name());
                    line = clean(in.readLine());
                }
                if (line == null) break;
                if (line.isBlank()) continue;

                CommandParser.Input input = parser.parse(line);
                if (input instanceof CommandParser.Slash s) {
                    if (s.name().equals("exit") || s.name().equals("quit")) break;
                    handleSlash(s);
                } else if (input instanceof CommandParser.Chat c) {
                    if (c.text().isBlank()) {
                        ui.error("보낼 내용이 없다.");
                        continue;
                    }
                    execute(resolveTargets(c.targets()), c.text(), false);
                }
            }
        }
        repo.saveMeta(room);
        if (tui != null) tui.restore();
        ui.notice("기록 저장: " + room.transcript());
    }

    // ---------- 라운드 실행 ----------

    /** 한 라운드를 실행하고 저장한다. 일반 채팅과 /run·/preset run 이 공유한다. */
    private void execute(List<AiProvider> targets, String text, boolean write) throws IOException {
        if (targets.isEmpty()) {
            ui.error("호출 가능한 참여자가 없다. /status 로 확인하라.");
            return;
        }
        int round = room.startRound();
        room.addUser(text);
        Path runDir = repo.runDir(room, round);

        // 문맥은 사용자 메시지를 저장한 뒤 조립한다 — 이번 요청도 포함되어야 한다.
        String prompt;
        try {
            prompt = PromptContextBuilder.build(room, text, "참여자");
        } catch (PromptContextBuilder.RequestTooLargeException e) {
            // 잘라 보내지 않는다 (SPEC §7.2 전송 규약).
            ui.error(e.getMessage());
            ui.notice("요청을 나눠서 보내라.");
            return;
        }

        if (write) {
            ui.notice("쓰기 프로필로 1회 실행한다 — 대상 " + targets.get(0).displayName()
                    + ", 워크스페이스 " + room.workspace());
        }
        if (tui != null) {
            tui.startRound(text, targets.stream().map(AiProvider::id).toList());
            refreshSideInfo();
            tui.draw();
        } else {
            ui.running(targets.stream().map(AiProvider::displayName).toList());
        }
        List<ProviderResult> results = runWatchingForCancel(targets, prompt, runDir, write);
        lastResults = results;
        if (tui != null) {
            results.forEach(tui::update);
            refreshSideInfo();
            tui.draw();
        } else {
            ui.roundSummary(results);
        }

        for (ProviderResult r : results) {
            room.add(r.providerId(), r.status().name(), r.elapsed().toMillis(),
                    r.text().isBlank() ? "(응답 없음)" : r.text());
        }
        repo.saveMeta(room);

        if (write) reportWorkspaceChanges();

        if (results.stream().noneMatch(ProviderResult::ok)) {
            ui.error("모든 참여자가 실패했다. /status 로 설치·인증 상태를 확인하라.");
        }
    }

    /**
     * 라운드를 백그라운드에서 돌리면서 stdin 을 폴링한다.
     *
     * 이렇게 하지 않으면 라운드가 도는 동안 입력 루프가 막혀 /cancel 을 칠 수 없다.
     * §7.10 의 "/cancel 이 대상 프로세스를 종료한다" 를 실제로 만족시키려면
     * 실행 중에도 입력을 받아야 한다.
     */
    private List<ProviderResult> runWatchingForCancel(List<AiProvider> targets, String prompt,
                                                      Path runDir, boolean write) {
        java.util.concurrent.CompletableFuture<List<ProviderResult>> f =
                java.util.concurrent.CompletableFuture.supplyAsync(() ->
                        executor.run(targets, prompt, room.workspace(), write,
                                runDir, repo.tempDir(), TIMEOUT, r -> {
                                    if (tui != null) {
                                        tui.update(r);
                                        tui.draw();
                                    }
                                }));
        try {
            while (!f.isDone()) {
                if (input != null && input.ready()) {
                    String line = clean(input.readLine());
                    if (line == null) break;
                    if (line.strip().startsWith("/cancel")) {
                        List<String> a = new ArrayList<>(
                                List.of(line.strip().split("[ \t]+")));
                        a.remove(0);
                        cancel(a);
                    } else if (!line.isBlank()) {
                        // 버리지 않는다. 라운드가 끝나면 순서대로 처리한다.
                        pending.addLast(line);
                    }
                }
                Thread.sleep(120);
            }
            return f.get();
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            targets.forEach(t -> launcher.cancel(t.id()));
            return List.of();
        } catch (java.io.IOException | java.util.concurrent.ExecutionException e) {
            ui.error("라운드 실행 오류: " + e.getMessage());
            return List.of();
        }
    }

    /** 왼쪽 dir / session 칸 내용을 갱신한다. */
    private void refreshSideInfo() {
        if (tui == null) return;
        tui.setWorkspace(room.workspace().toString());
        tui.setSession(room.name(),
                "라운드 " + room.round()
                        + "\n메시지 " + room.messages().size()
                        + "\n문맥 " + PromptContextBuilder.maxMessages() + "개");
    }

    /** /s <번호|이름> <줄> — 칸 안에서 스크롤한다. 단일 키 입력을 못 받아 줄 단위다. */
    private void scroll(List<String> args) {
        if (tui == null) {
            ui.error("평문 모드에서는 스크롤이 없다. /v 로 전체를 본다.");
            return;
        }
        if (args.size() < 2) {
            ui.error("사용법: /s <번호|이름> <줄번호>   예) /s claude 40");
            return;
        }
        var p = tui.pane(args.get(0));
        if (p.isEmpty()) {
            ui.error("그런 칸이 없다: " + args.get(0));
            return;
        }
        try {
            p.get().scroll = Math.max(0, Integer.parseInt(args.get(1)));
        } catch (NumberFormatException e) {
            ui.error("줄 번호가 숫자가 아니다.");
            return;
        }
        tui.draw();
    }

    /** /n — 질문 전 화면으로 되돌린다. */
    private void resetScreen() {
        if (tui == null) {
            ui.error("평문 모드에서는 쓰지 않는다.");
            return;
        }
        tui.startRound("", List.of());
        lastResults = List.of();
        tui.setHint("");
        // started 를 끄기 위해 새 라운드 상태를 비우고 다시 그린다.
        tui.panes().forEach(x -> { x.body = ""; x.status = "대기"; x.elapsed = ""; });
        tui.draw();
    }

    /**
     * /v [번호|all] — 직전 라운드의 답변 구역을 펼쳐 본다.
     *
     * 세 답변을 한꺼번에 쏟으면 읽을 수가 없어서, 라운드가 끝나면 요약표만 보여주고
     * 여기서 원하는 것만 골라 보게 한다.
     */
    private void view(List<String> args) {
        if (lastResults.isEmpty()) {
            ui.error("아직 답변이 없다. 먼저 질문을 하라.");
            return;
        }
        if (args.isEmpty()) {
            if (tui != null) tui.draw(); else ui.roundSummary(lastResults);
            return;
        }
        if (tui != null) {
            var p = tui.pane(args.get(0).replaceFirst("^@", ""));
            if (p.isPresent()) {
                p.get().scroll = 0;
                tui.drawSingle(p.get());
                return;
            }
        }
        String a = args.get(0).toLowerCase(Locale.ROOT);
        if (a.equals("all") || a.equals("a")) {
            for (int i = 0; i < lastResults.size(); i++) ui.answer(i + 1, lastResults.get(i));
            return;
        }
        // 번호로도, 참여자 이름으로도 고를 수 있게 한다.
        String id = a.replaceFirst("^@", "");
        for (int i = 0; i < lastResults.size(); i++) {
            ProviderResult r = lastResults.get(i);
            if (String.valueOf(i + 1).equals(a) || r.providerId().equals(id)) {
                ui.answer(i + 1, r);
                return;
            }
        }
        ui.error("그런 답변이 없다: " + a + "   (/v 로 목록 확인)");
    }

    /**
     * /context [메시지수] — 프롬프트에 실을 이전 대화 분량을 조절한다.
     *
     * 토큰이 나가는 지점은 이쪽이다. transcript.md 저장은 로컬 디스크 쓰기라
     * 토큰과 무관하다.
     */
    private void context(List<String> args) {
        if (args.isEmpty()) {
            ui.blank();
            ui.print("  이전 대화 포함   최근 " + PromptContextBuilder.maxMessages() + "개 메시지");
            ui.print("  프롬프트 상한    " + String.format("%,d", PromptContextBuilder.maxChars()) + "자");
            ui.blank();
            ui.notice("/context 0    이전 대화를 안 넣는다 (매 질문이 독립 질의)");
            ui.notice("/context 4    최근 4개만 넣는다");
            ui.notice("토큰은 여기서 나간다. 기록 저장(transcript.md)은 토큰과 무관하다.");
            ui.blank();
            return;
        }
        try {
            int n = Integer.parseInt(args.get(0));
            PromptContextBuilder.configure(n, PromptContextBuilder.maxChars());
            ui.notice(n == 0
                    ? "이전 대화를 넣지 않는다. 매 질문이 독립 질의가 된다."
                    : "이전 대화를 최근 " + n + "개까지 넣는다.");
            if (n == 0) {
                ui.notice("다른 AI 의 답을 읽고 답하는 협업 구조는 꺼진다 (INTENT.md §5).");
            }
        } catch (NumberFormatException e) {
            ui.error("사용법: /context [메시지수]   예) /context 4");
        }
    }

    /** 멘션이 없으면 전 참여자. 있으면 지목된 참여자만. */
    private List<AiProvider> resolveTargets(List<String> mentions) {
        if (mentions.isEmpty()) return providers;
        List<AiProvider> out = new ArrayList<>();
        for (AiProvider p : providers) if (mentions.contains(p.id())) out.add(p);
        return out;
    }

    // ---------- 슬래시 명령 ----------

    private void handleSlash(CommandParser.Slash s) throws IOException {
        switch (s.name()) {
            case "status" -> status(!s.args().isEmpty()
                    && s.args().get(0).equalsIgnoreCase("auth"));
            case "new" -> newRoom(String.join(" ", s.args()));
            case "rooms" -> rooms();
            case "open" -> open(s.args());
            case "cancel" -> cancel(s.args());
            case "run" -> runCommand(s);
            case "preset" -> preset(s);
            case "converge" -> converge(s);
            case "v", "view" -> view(s.args());
            case "s", "scroll" -> scroll(s.args());
            case "n" -> resetScreen();
            case "context" -> context(s.args());
            case "help" -> banner();
            default -> ui.error("알 수 없는 명령: /" + s.name() + "  (/help 로 목록 확인)");
        }
    }

    /**
     * /run [멘션] [--write] [프롬프트]. SPEC §7.5 「쓰기 실행」.
     * 쓰기 프로필은 이 호출 한 번에만 적용되고 세션에 남지 않는다.
     */
    private void runCommand(CommandParser.Slash s) throws IOException {
        RunCommand rc;
        try {
            rc = RunCommand.parse(s.args(), s.raw(), providerIds());
        } catch (RunCommand.ParseException e) {
            ui.error(e.getMessage());
            return;
        }
        execute(resolveTargets(rc.targets()), rc.prompt(), rc.write());
    }

    /**
     * /preset save [이름] [프롬프트] | run [이름] | list | rm [이름]. SPEC §7.5.
     * 프리셋 실행은 쓰기 권한을 승격하지 않는다 (§7.10 완료 기준).
     */
    private void preset(CommandParser.Slash s) throws IOException {
        List<String> a = s.args();
        String sub = a.isEmpty() ? "list" : a.get(0).toLowerCase(Locale.ROOT);
        switch (sub) {
            case "list" -> presetList();
            case "save" -> presetSave(s, a);
            case "run" -> presetRun(a);
            case "rm" -> presetRemove(a);
            default -> ui.error("사용법: /preset [list|save|run|rm] ...");
        }
    }

    private void presetList() {
        if (presets.all().isEmpty()) {
            ui.notice("저장된 프리셋이 없다.  /preset save <이름> [@참여자...] <프롬프트>");
            return;
        }
        ui.blank();
        for (PromptPresetRepository.Preset x : presets.all()) {
            String tg = x.targets().isEmpty() ? "전체" : String.join(",", x.targets());
            ui.print("  " + pad(x.name(), 16) + pad("[" + tg + "]", 20) + preview(x.prompt()));
        }
        ui.blank();
    }

    private void presetSave(CommandParser.Slash s, List<String> a) throws IOException {
        if (a.size() < 2) {
            ui.error("사용법: /preset save <이름> [@참여자...] <프롬프트>");
            return;
        }
        String name = a.get(1);
        String rest = stripTokens(s.raw(), List.of(a.get(0), name));
        List<String> targets = new ArrayList<>();
        while (rest.startsWith("@")) {
            int sp = rest.indexOf(' ');
            String id = (sp < 0 ? rest : rest.substring(0, sp))
                    .substring(1).toLowerCase(Locale.ROOT);
            if (!providerIds().contains(id)) break;
            if (!targets.contains(id)) targets.add(id);
            rest = sp < 0 ? "" : rest.substring(sp + 1).stripLeading();
        }
        if (rest.isBlank()) {
            ui.error("프롬프트가 비어 있다.");
            return;
        }
        presets.save(new PromptPresetRepository.Preset(name, targets, rest));
        ui.notice("프리셋 저장: " + name + (targets.isEmpty() ? " [전체]" : " " + targets));
    }

    private void presetRun(List<String> a) throws IOException {
        if (a.size() < 2) {
            ui.error("사용법: /preset run <이름>");
            return;
        }
        Optional<PromptPresetRepository.Preset> found = presets.get(a.get(1));
        if (found.isEmpty()) {
            ui.error("프리셋을 찾을 수 없다: " + a.get(1));
            return;
        }
        // 프리셋은 권한을 저장하지 않는다. 언제나 읽기 전용으로 실행한다.
        PromptPresetRepository.Preset x = found.get();
        execute(resolveTargets(x.targets()), x.prompt(), false);
    }

    private void presetRemove(List<String> a) throws IOException {
        if (a.size() < 2) {
            ui.error("사용법: /preset rm <이름>");
            return;
        }
        ui.notice(presets.remove(a.get(1))
                ? "삭제됨: " + a.get(1)
                : "프리셋을 찾을 수 없다: " + a.get(1));
    }

    // ---------- Phase 3: 구조화 수렴 ----------

    /**
     * /converge [@수렴자] 안건. SPEC §7.5-1 · §7.9.
     *
     * 수렴자로 지명된 참여자는 검토 대상에서 제외된다 — 자기 답을 자기가 분류하면
     * 자기 채점이 된다. 지명이 없으면 규칙 기반 분류만 수행한다.
     */
    private void converge(CommandParser.Slash s) throws IOException {
        List<String> a = s.args();
        String consolidator = null;
        int skip = 0;
        if (!a.isEmpty() && a.get(0).startsWith("@")) {
            String id = a.get(0).substring(1).toLowerCase(Locale.ROOT);
            if (!providerIds().contains(id)) {
                ui.error("알 수 없는 수렴자: @" + id);
                return;
            }
            consolidator = id;
            skip = 1;
        }
        String subject = stripTokens(s.raw(), a.subList(0, skip));
        if (subject.isBlank()) {
            ui.error("사용법: /converge [@수렴자] <안건>");
            ui.notice("수렴자를 지명하면 그 참여자는 검토에서 빠진다 (§7.5-1).");
            return;
        }

        final String cons = consolidator;
        List<AiProvider> reviewers = providers.stream()
                .filter(p -> cons == null || !p.id().equals(cons)).toList();
        if (reviewers.size() < 2) {
            ui.error("검토자가 2명 미만이다. 수렴에는 최소 2명이 필요하다.");
            return;
        }

        int round = room.startRound();
        room.addUser("[converge] " + subject);
        Path outDir = repo.runDir(room, round).resolve("converge");

        ui.blank();
        ui.notice("검토자: " + reviewers.stream().map(AiProvider::displayName).toList());
        ui.notice(cons == null ? "수렴자: 규칙 기반 (모델 호출 없음)"
                : "수렴자: " + cons + " (검토에서 제외됨)");

        ConvergeSession session = new ConvergeSession(executor, repo.tempDir(), TIMEOUT);
        ConvergeSession.Result res = session.run(reviewers, subject, room.workspace(), outDir,
                new ConvergeSession.Progress() {
                    @Override public void stage(String m) { ui.notice(m); }
                    @Override public void reviewerDone(StructuredReview r) {
                        ui.print("  [" + r.reviewerName() + "] " + r.verdict()
                                + " · 지적 " + r.issues().size() + "건");
                    }
                });

        if (res.aborted()) {
            ui.error(res.abortReason());
            repo.saveMeta(room);
            return;
        }
        printConvergeSummary(res);
        room.add("consolidator", "OK", 0, "수렴 보고서: " + res.report());
        repo.saveMeta(room);
    }

    /** 터미널에는 판정 요약과 미해결만 낸다. 전문은 경로만 안내한다 (§7.5-1). */
    private void printConvergeSummary(ConvergeSession.Result res) {
        ConsolidationEngine.Outcome last = res.round2() != null ? res.round2() : res.round1();
        ui.blank();
        ui.print("== 판정 요약 ==");
        for (StructuredReview r : last.reviews()) {
            ui.print("  " + pad(r.reviewerName(), 18)
                    + (r.valid() ? r.verdict().toString() : "응답 없음"));
        }
        // 1라운드와 2라운드의 실패를 모두 알린다 — last 만 보면 1라운드 실패를,
        // round1 만 보면 2라운드 실패를 놓친다.
        java.util.LinkedHashSet<String> failed =
                new java.util.LinkedHashSet<>(res.round1().failedReviewers());
        if (res.round2() != null) failed.addAll(res.round2().failedReviewers());
        if (!failed.isEmpty()) {
            ui.error("PARTIAL — 응답 없음: " + String.join(", ", failed));
        }

        long agree = last.findings().stream()
                .filter(f -> f.bucket() == ConsolidationEngine.Bucket.합의).count();
        long dis = last.findings().stream()
                .filter(f -> f.bucket() == ConsolidationEngine.Bucket.이견).count();
        long solo = last.findings().stream()
                .filter(f -> f.bucket() == ConsolidationEngine.Bucket.단독지적).count();
        ui.blank();
        ui.print("  합의 " + agree + " · 이견 " + dis + " · 단독 지적 " + solo
                + " · 미해결 " + last.openQuestions().size());

        if (!last.openQuestions().isEmpty()) {
            ui.blank();
            ui.print("== 사용자 결정 필요 ==");
            last.openQuestions().forEach(q -> ui.print("  - " + q));
        }
        ui.blank();
        ui.notice("보고서: " + res.report());
        ui.blank();
    }

    /** SPEC §7.5 — 쓰기 실행 후 변경 목록만 보여준다. 커밋·푸시는 하지 않는다. */
    private void reportWorkspaceChanges() {
        WorkspaceStatus.Report rep = WorkspaceStatus.collect(room.workspace());
        ui.blank();
        ui.print("== 워크스페이스 변경 (" + rep.vcs() + ") ==");
        if (!rep.note().isBlank()) ui.notice(rep.note());
        if (rep.hasChanges()) {
            rep.lines().forEach(l -> ui.print("  " + l));
            ui.blank();
            ui.notice("커밋·푸시는 수행하지 않는다. 필요하면 직접 실행하라.");
        } else if (rep.vcs() != WorkspaceStatus.Vcs.NONE) {
            ui.notice("변경 없음");
        }
        ui.blank();
    }

    // ---------- 방 관리 ----------

    /**
     * @param probe true 면 각 CLI 를 실제로 호출해 인증 가능 여부를 확인한다.
     *              SPEC §7.10 — agy 는 사용 가능한 Gemini 모델도 하나 이상 확인한다.
     */
    private void status(boolean probe) {
        ui.blank();
        ui.print("== 참여자 ==");
        for (AiProvider p : providers) {
            ResolvedCommand c = p.command();
            String tier = c.isPrimary() ? "" : "  [강등 tier " + c.tier() + "]";
            ui.print("  " + pad(p.displayName(), 18) + c.executable() + tier);
            if (!c.note().isBlank()) ui.print("      " + c.note());
            if (probe) {
                ui.print("      버전   " + probeVersion(c));
                ui.print("      인증   " + probeAuth(p, c));
            }
        }
        if (!probe) ui.notice("인증·모델 확인은 /status auth");
        ui.blank();
        ui.print("== 방 ==");
        ui.print("  id        " + room.id());
        ui.print("  이름      " + room.name());
        ui.print("  워크스페이스 " + room.workspace());
        ui.print("  라운드    " + room.round() + " / 메시지 " + room.messages().size());
        long dmg = room.damagedCount();
        if (dmg > 0) ui.error("복원 시 손상·의심 메시지 " + dmg + "건 (SUSPECT/CORRUPT)");
        ui.print("  저장 위치 " + repo.home());
        ui.print("  프리셋    " + presets.all().size() + "건");
        ui.blank();
    }

    private String probeVersion(ResolvedCommand c) {
        List<String> cmd = new ArrayList<>(c.launcher());
        cmd.add("--version");
        String out = runProbe(cmd, 20);
        return out.isBlank() ? "확인 실패" : out.lines().findFirst().orElse("").strip();
    }

    /**
     * 인증 가능 여부. agy 는 models 조회로 실제 사용 가능한 Gemini 모델까지 센다.
     * claude·codex 는 버전 조회만으로는 인증을 알 수 없어 미확인으로 표시한다 —
     * 짧은 실호출은 쿼터를 쓰므로 /status 에서 하지 않는다.
     */
    private String probeAuth(AiProvider p, ResolvedCommand c) {
        if (!p.id().equals("gemini")) {
            return "미확인 (첫 호출에서 판정)";
        }
        List<String> cmd = new ArrayList<>(c.launcher());
        cmd.add("models");
        String out = runProbe(cmd, 60);
        long gemini = out.lines().filter(l -> l.startsWith("gemini-")).count();
        if (gemini == 0) return "실패 — 모델 조회 불가. 재로그인이 필요할 수 있다";
        return "정상 · Gemini 모델 " + gemini + "종 사용 가능";
    }

    /** 짧은 조회 명령을 돌려 stdout 을 얻는다. 실패는 빈 문자열이다. */
    private String runProbe(List<String> cmd, int seconds) {
        try {
            Process pr = new ProcessBuilder(cmd)
                    .directory(room.workspace().toFile())
                    .redirectErrorStream(true).start();
            pr.getOutputStream().close();
            byte[] b = pr.getInputStream().readAllBytes();
            if (!pr.waitFor(seconds, java.util.concurrent.TimeUnit.SECONDS)) {
                pr.destroyForcibly();
                return "";
            }
            return new String(b, StandardCharsets.UTF_8);
        } catch (IOException e) {
            return "";
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return "";
        }
    }

    private void newRoom(String name) throws IOException {
        repo.saveMeta(room);
        room = repo.create(name, room.workspace());
        ui.notice("새 방: " + room.name() + " (" + room.id() + ")");
    }

    private void rooms() throws IOException {
        List<RoomRepository.RoomSummary> list = repo.list();
        if (list.isEmpty()) {
            ui.notice("저장된 방이 없다.");
            return;
        }
        ui.blank();
        for (RoomRepository.RoomSummary r : list) {
            String cur = r.id().equals(room.id()) ? " *" : "";
            ui.print("  " + pad(r.id(), 18) + pad(r.name(), 24) + "라운드 " + r.round() + cur);
        }
        ui.blank();
    }

    private void open(List<String> args) throws IOException {
        if (args.isEmpty()) {
            ui.error("사용법: /open <방 ID>   (/rooms 로 목록 확인)");
            return;
        }
        try {
            repo.saveMeta(room);
            ChatRoom opened = repo.open(args.get(0));
            // SPEC D18-5 — 저장된 워크스페이스가 없어졌으면 조용히 대체하지 않는다.
            if (!Files.isDirectory(opened.workspace())) {
                ui.error("저장된 워크스페이스가 존재하지 않는다: " + opened.workspace());
                ui.notice("이 방은 열지 않는다. 경로를 복구하거나 새 방을 만들어라.");
                return;
            }
            room = opened;
            ui.notice("방 열기: " + room.name() + "  메시지 " + room.messages().size() + "건");
            long dmg = room.damagedCount();
            if (dmg > 0) {
                ui.error("복원 중 손상·의심 " + dmg + "건. 해당 메시지는 문맥에서 제외되거나 의심 표시된다.");
            }
        } catch (NoSuchFileException e) {
            ui.error("방을 찾을 수 없다: " + args.get(0));
        }
    }

    /** SPEC §6.3 — 지정 참여자의 프로세스 트리만 종료한다. best-effort 다. */
    private void cancel(List<String> args) {
        if (args.isEmpty()) {
            for (AiProvider p : providers) launcher.cancel(p.id());
            ui.notice("전체 취소 요청. 생존 프로세스가 있으면 오류로 표시된다.");
            return;
        }
        String id = args.get(0).replaceFirst("^@", "").toLowerCase(Locale.ROOT);
        launcher.cancel(id);
        ui.notice("취소 요청: " + id);
    }

    private void banner() {
        ui.blank();
        ui.print("multi_ai_cli — Claude · Codex · Gemini(agy) 한 방에서");
        ui.print("  일반 문장          전 참여자 동시 호출");
        ui.print("  @claude/@codex/@gemini <질문>   지목 호출");
        ui.print("  /run @<참여자> [--write] <프롬프트>   지정 권한으로 1회 실행");
        ui.print("  /preset [list|save|run|rm] ...   프롬프트 프리셋 (권한 승격 없음)");
        ui.print("  /converge [@수렴자] <안건>   구조화 교차검증 → REPORT.md");
        ui.print("  /v <번호|all>      답변 구역 펼쳐 보기");
        ui.print("  /context [n]       프롬프트에 실을 이전 대화 분량 (토큰 조절)");
        ui.print("  /status [auth] /rooms /open <ID> /new [이름] /cancel [참여자] /exit");
        ui.blank();
    }

    // ---------- 헬퍼 ----------

    /**
     * 읽은 입력 줄을 정리한다.
     *
     * 스트림 선두의 BOM(U+FEFF)을 걷어낸다 — PowerShell 이 네이티브 프로세스로
     * 파이프할 때 UTF-8 BOM 을 붙이는 경우가 있어, 그대로 두면 첫 줄의 "/status" 가
     * "﻿/status" 가 되어 슬래시 명령으로 인식되지 않는다 (실측).
     */
    private static String clean(String line) {
        if (line == null) return null;
        int i = 0;
        while (i < line.length() && line.charAt(i) == '﻿') i++;
        return i == 0 ? line : line.substring(i);
    }

    private static String pad(String s, int n) {
        return s.length() >= n ? s + " " : s + " ".repeat(n - s.length());
    }

    private static String preview(String s) {
        String one = s.replace('\n', ' ').strip();
        return one.length() <= 40 ? one : one.substring(0, 40) + "...";
    }

    /** raw 앞쪽에서 지정 토큰들을 순서대로 걷어낸다. */
    private static String stripTokens(String raw, List<String> tokens) {
        String rest = raw;
        for (String t : tokens) {
            int idx = rest.indexOf(t);
            if (idx < 0) break;
            rest = rest.substring(idx + t.length()).stripLeading();
        }
        return rest.strip();
    }

    @Override
    public void close() {
        executor.close();
        launcher.shutdown();
    }
}
