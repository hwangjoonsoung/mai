package io.multiai.cli.room;

import java.io.*;
import java.nio.ByteBuffer;
import java.nio.charset.*;
import java.nio.file.*;
import java.time.Instant;
import java.util.*;

/**
 * transcript.md 직렬화·역직렬화. SPEC §7.6 「메시지 경계 규격」.
 *
 * 여는 마커에 본문의 UTF-8 바이트 길이를 기록하고 읽을 때 정확히 그만큼만 읽는다.
 * 본문에는 어떤 이스케이프도 하지 않으므로 마커처럼 생긴 문자열이 들어와도 안전하다.
 *
 * 디스크 개행은 LF(0x0A) 하나로 고정한다. PrintWriter·println 을 쓰지 않는다 —
 * CRLF 가 섞이면 bytes 계산과 닫는 마커 매칭이 1바이트씩 어긋난다 (교차검토 C1).
 */
public final class TranscriptCodec {

    private static final byte[] LF = {0x0A};
    private static final String OPEN = "<!-- msg ";

    private TranscriptCodec() {}

    // ---------- 쓰기 ----------

    public static void append(Path file, ChatMessage m) throws IOException {
        Files.write(file, frame(m), StandardOpenOption.CREATE, StandardOpenOption.APPEND);
    }

    /** 전체를 임시 파일에 쓴 뒤 원자적으로 교체한다. */
    public static void writeAll(Path file, List<ChatMessage> msgs) throws IOException {
        Path tmp = file.resolveSibling(file.getFileName() + ".tmp");
        try (OutputStream o = new BufferedOutputStream(Files.newOutputStream(tmp,
                StandardOpenOption.CREATE, StandardOpenOption.TRUNCATE_EXISTING))) {
            for (ChatMessage m : msgs) o.write(frame(m));
        }
        try {
            Files.move(tmp, file, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException e) {
            Files.move(tmp, file, StandardCopyOption.REPLACE_EXISTING);
        }
    }

    private static byte[] frame(ChatMessage m) throws IOException {
        byte[] body = m.body().getBytes(StandardCharsets.UTF_8);
        String header = String.format(
                "<!-- msg id=%04d round=%d sender=%s ts=%s status=%s ms=%d bytes=%d -->",
                m.id(), m.round(), m.sender(), m.ts(), m.status(), m.ms(), body.length);
        ByteArrayOutputStream o = new ByteArrayOutputStream();
        o.write(header.getBytes(StandardCharsets.UTF_8));
        o.write(LF);
        o.write(body);
        o.write(LF);
        o.write(String.format("<!-- /msg id=%04d -->", m.id()).getBytes(StandardCharsets.UTF_8));
        o.write(LF);
        return o.toByteArray();
    }

    // ---------- 읽기 ----------

    /**
     * SPEC §7.6 「읽기 절차」. 손상되지 않은 파일은 무손실 왕복을 보장한다.
     * 손상 이후 재동기화는 best-effort 이며 복구분을 SUSPECT 로 표시한다.
     *
     * @param nextId room.properties 의 next_id. 재동기화 조건 5 의 상한이다.
     */
    public static List<ChatMessage> readAll(Path file, int nextId) throws IOException {
        if (!Files.exists(file)) return List.of();
        byte[] all = Files.readAllBytes(file);
        List<ChatMessage> out = new ArrayList<>();
        int p = 0;
        int lastId = 0;
        boolean resyncing = false;

        while (p < all.length) {
            int nl = indexOf(all, LF, p);
            if (nl < 0) break;
            Frame f = parseHeader(all, p, nl);

            boolean ok = f != null;
            if (ok && resyncing) {
                // 재동기화 조건 4·5. 조건 4 는 단조 증가만 요구한다 — id == last+1 로
                // 강화하면 손상으로 id 가 건너뛴 뒤 모든 후속 메시지를 잃는다 (교차검토 E2).
                ok = f.id() > lastId && f.id() < nextId;
            }
            int bodyStart = nl + 1;
            byte[] close = ok ? closeMarker(f.id()) : null;
            if (ok && (bodyStart + f.bytes() > all.length
                    || !regionMatches(all, bodyStart + f.bytes(), close))) {
                ok = false;
            }
            if (!ok) {
                if (!resyncing) {
                    out.add(corrupt(lastId + 1));
                    resyncing = true;
                }
                p = nl + 1;   // 다음 줄부터 후보 재탐색
                continue;
            }

            ChatMessage.State state = resyncing ? ChatMessage.State.SUSPECT : ChatMessage.State.OK;
            String body;
            try {
                body = strictDecode(all, bodyStart, f.bytes());
            } catch (CharacterCodingException e) {
                out.add(corrupt(f.id()));
                lastId = f.id();
                resyncing = true;
                p = bodyStart + f.bytes() + close.length;
                continue;
            }
            out.add(new ChatMessage(f.id(), f.round(), f.sender(), f.ts(),
                    f.status(), f.ms(), body, state));
            lastId = f.id();
            resyncing = false;
            p = bodyStart + f.bytes() + close.length;
        }
        return out;
    }

    private record Frame(int id, int round, String sender, Instant ts,
                         String status, long ms, int bytes) {}

    private static Frame parseHeader(byte[] all, int from, int nl) {
        String h = new String(all, from, nl - from, StandardCharsets.UTF_8);
        if (!h.startsWith(OPEN) || !h.endsWith("-->")) return null;
        Map<String, String> kv = new HashMap<>();
        String inner = h.substring(OPEN.length(), h.length() - 3).trim();
        for (String tok : inner.split("[ \\t]+")) {
            int eq = tok.indexOf('=');
            if (eq > 0) kv.put(tok.substring(0, eq), tok.substring(eq + 1));
        }
        try {
            return new Frame(
                    Integer.parseInt(kv.get("id")),
                    Integer.parseInt(kv.getOrDefault("round", "0")),
                    kv.getOrDefault("sender", "unknown"),
                    Instant.parse(kv.getOrDefault("ts", Instant.EPOCH.toString())),
                    kv.getOrDefault("status", "OK"),
                    Long.parseLong(kv.getOrDefault("ms", "0")),
                    Integer.parseInt(kv.get("bytes")));
        } catch (RuntimeException e) {
            return null;
        }
    }

    /** strict 디코딩. 실패하면 CORRUPT 로 표시한다 — 원시 바이트는 runs/ 에 남는다. */
    private static String strictDecode(byte[] a, int off, int len) throws CharacterCodingException {
        return StandardCharsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(a, off, len)).toString();
    }

    private static ChatMessage corrupt(int id) {
        return new ChatMessage(id, 0, "unknown", Instant.EPOCH, "CORRUPT", 0, "",
                ChatMessage.State.CORRUPT);
    }

    private static byte[] closeMarker(int id) {
        return ("\n<!-- /msg id=" + String.format("%04d", id) + " -->\n")
                .getBytes(StandardCharsets.UTF_8);
    }

    private static boolean regionMatches(byte[] a, int off, byte[] n) {
        if (off + n.length > a.length) return false;
        for (int i = 0; i < n.length; i++) if (a[off + i] != n[i]) return false;
        return true;
    }

    private static int indexOf(byte[] a, byte[] n, int from) {
        outer:
        for (int i = from; i <= a.length - n.length; i++) {
            for (int j = 0; j < n.length; j++) if (a[i + j] != n[j]) continue outer;
            return i;
        }
        return -1;
    }
}
