package io.multiai.cli.orchestration;

import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;

/**
 * 사용자 정의 프롬프트 프리셋. SPEC §7.5.
 *
 * - 프리셋은 단순한 프롬프트 텍스트와 대상 멘션만 저장한다.
 * - planner / implementer / reviewer 같은 고정 역할을 제품에 내장하지 않는다.
 * - **프리셋을 실행해도 쓰기 권한은 자동 승격하지 않는다.** 쓰기는 언제나
 *   /run --write 를 명시해야 한다 (§7.10 완료 기준).
 */
public final class PromptPresetRepository {

    /** @param targets 비어 있으면 전 참여자 */
    public record Preset(String name, List<String> targets, String prompt) {}

    private final Path file;
    private final Map<String, Preset> cache = new LinkedHashMap<>();

    public PromptPresetRepository(Path file) {
        this.file = file;
    }

    public void load() throws IOException {
        cache.clear();
        if (!Files.exists(file)) return;
        Properties p = new Properties();
        try (Reader r = Files.newBufferedReader(file, StandardCharsets.UTF_8)) {
            p.load(r);
        }
        for (String key : p.stringPropertyNames()) {
            if (!key.endsWith(".prompt")) continue;
            String name = key.substring(0, key.length() - ".prompt".length());
            String targets = p.getProperty(name + ".targets", "").trim();
            cache.put(name, new Preset(name,
                    targets.isEmpty() ? List.of() : List.of(targets.split(",")),
                    p.getProperty(key, "")));
        }
    }

    public Optional<Preset> get(String name) {
        return Optional.ofNullable(cache.get(name));
    }

    public Collection<Preset> all() {
        return Collections.unmodifiableCollection(cache.values());
    }

    public void save(Preset preset) throws IOException {
        cache.put(preset.name(), preset);
        Properties p = new Properties();
        for (Preset x : cache.values()) {
            p.setProperty(x.name() + ".prompt", x.prompt());
            p.setProperty(x.name() + ".targets", String.join(",", x.targets()));
        }
        Files.createDirectories(file.getParent());
        try (Writer w = Files.newBufferedWriter(file, StandardCharsets.UTF_8)) {
            p.store(w, "multi_ai_cli prompt presets — 권한을 저장하지 않는다");
        }
    }

    public boolean remove(String name) throws IOException {
        if (cache.remove(name) == null) return false;
        save0();
        return true;
    }

    private void save0() throws IOException {
        Properties p = new Properties();
        for (Preset x : cache.values()) {
            p.setProperty(x.name() + ".prompt", x.prompt());
            p.setProperty(x.name() + ".targets", String.join(",", x.targets()));
        }
        try (Writer w = Files.newBufferedWriter(file, StandardCharsets.UTF_8)) {
            p.store(w, "multi_ai_cli prompt presets");
        }
    }
}
