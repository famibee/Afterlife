// 親機のネットワーク受信をRust本体から切り出したサイドカー。
// 未署名のRustネイティブバイナリが外部からの着信TCPを一部のセキュリティソフト(確認済み: Avast)に
// 無条件で遮断される一方、安定した実行ファイルである`bun`インタプリタ経由のWebSocketサーバーは
// 遮断されないことを実機で確認済みのため、この部分だけBunで実装する。
//
// Rust本体とはstdin/stdoutのJSON Lines(改行区切りJSON)でやり取りする。ネットワークは一切使わない。
//
// Rust → Bun (stdin, 1行1コマンド):
//   {"cmd":"broadcast","data":"<ServerMsgのJSON文字列>"}
//
// Bun → Rust (stdout, 1行1イベント):
//   {"event":"open","id":<number>}
//   {"event":"message","id":<number>,"data":"<受信した生のJSON文字列>"}
//   {"event":"close","id":<number>}
//   {"event":"listening","port":<number>}
//   {"event":"error","message":"<string>"}

const port = Number(process.env.AFTERLIFE_PORT ?? "47821");

let nextId = 1;
const sockets = new Map<number, import("bun").ServerWebSocket<{ id: number }>>();

function emit(obj: unknown) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

try {
  Bun.serve({
    port,
    hostname: "0.0.0.0",
    fetch(req, server) {
      const url = new URL(req.url);
      // デバッグ専用: 127.0.0.1からのみ受け付け、全クライアントへブロードキャストする。
      // 画面操作なしでテストクリックを自動発火させるための開発用エンドポイント。
      if (url.pathname === "/debug/send") {
        const remote = server.requestIP(req)?.address;
        if (remote !== "127.0.0.1" && remote !== "::1") {
          return new Response("forbidden\n", { status: 403 });
        }
        const data = url.searchParams.get("data");
        if (!data) {
          return new Response("missing data\n", { status: 400 });
        }
        for (const ws of sockets.values()) {
          ws.send(data);
        }
        emit({ event: "debug-sent", data, count: sockets.size });
        return new Response(`sent to ${sockets.size} client(s)\n`);
      }
      if (server.upgrade(req, { data: { id: nextId++ } })) {
        return;
      }
      return new Response("afterlife host sidecar\n");
    },
    websocket: {
      open(ws) {
        sockets.set(ws.data.id, ws);
        emit({ event: "open", id: ws.data.id });
      },
      message(ws, message) {
        if (typeof message !== "string") return;
        emit({ event: "message", id: ws.data.id, data: message });
      },
      close(ws) {
        sockets.delete(ws.data.id);
        emit({ event: "close", id: ws.data.id });
      },
    },
  });
  emit({ event: "listening", port });
} catch (e) {
  emit({ event: "error", message: String(e) });
  process.exit(1);
}

const decoder = new TextDecoder();
let buf = "";
for await (const chunk of Bun.stdin.stream()) {
  buf += decoder.decode(chunk, { stream: true });
  let idx: number;
  while ((idx = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, idx).trim();
    buf = buf.slice(idx + 1);
    if (!line) continue;
    try {
      const cmd = JSON.parse(line);
      if (cmd.cmd === "broadcast" && typeof cmd.data === "string") {
        for (const ws of sockets.values()) {
          ws.send(cmd.data);
        }
      }
    } catch (e) {
      emit({ event: "error", message: `failed to parse stdin command: ${e}` });
    }
  }
}
