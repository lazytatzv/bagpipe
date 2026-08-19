# 🎺 bagpipe (`bp`)

> **The ultra-fast, zero-friction ROS 2 bag pipeline tool.**  
> Record, zstd-compress, summarize, and ship ROS 2 bags directly to Discord in a single breath.

---

## ⚡ 最強にシンプルな使い方 (Ergonomic UX)

`bagpipe`（短縮コマンド: `bp`）はサブコマンドの指定すら省略でき、プロが最もよく行う操作を自動判定します。

### 1. 初回設定（これだけ）
```bash
bp --init "https://discord.com/api/webhooks/your/webhook/url"
```

### 2. ワンライナー録画 & 自動送信
`bp` の後ろに `-a` やトピック名を渡すだけで自動録画モードになります。
```bash
# 全トピック録画 -> Ctrl+C で停止 -> 即座に zstd 圧縮 & Discord へ送信！
bp -a

# トピック指定 & メモ付き
bp /camera/image_raw /cmd_vel -m "屋外障害物回避テスト"
```

### 3. 既存バッグの送信
```bash
# 今カレントディレクトリにある最新の rosbag を自動検出して Discord へ送信！
bp

# パス指定で送信
bp ./my_rosbag_dir

# 圧縮ファイル (.tar.zst) をローカルにも残す場合
bp ./my_rosbag_dir -k
```

### 4. メタデータサマリ確認（送信なし）
```bash
bp info ./my_rosbag_dir
# またはカレントの最新バッグを確認
bp info
```

---

## 💡 特徴

- **🚀 超軽量・爆速**: ピュア Rust 製（シングルバイナリ）。
- **🧠 賢い引数自動推論**:
  - `bp`（引数なし）➔ カレントディレクトリの最新の rosbag を検出して送信
  - `bp <ディレクトリ>` ➔ 既存バッグを圧縮して送信
  - `bp -a` または `bp /topic` ➔ `ros2 bag record` を開始し、停止後に自動送信
- **🗜️ 高速マルチスレッド zstd 圧縮**: `.tar.zst` で劇的にサイズ削減。
- **📊 丁寧な Embed サマリ**: 録画時間・開始時刻・メッセージ数・トピック内訳テーブルを Discord に見やすく整形。
- **🛡️ 25MB 上限保護**: Discord の上限を超える巨大バッグでも Webhook エラーで落ちず、サマリ通知＋ローカル保管パスを案内。

---

## 📦 インストール

```bash
cargo install --path .
```

---

## ⚙️ 設定

現在の設定を確認:
```bash
bp --config
```
設定ファイル: `~/.config/bagpipe/config.json`

---

## 📄 License
MIT OR Apache-2.0
