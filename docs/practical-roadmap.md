# agent-handoff 実用化仕様(v0.3〜v0.5 ロードマップ)

本ドキュメントは、現状の MVP(v0.2.0 時点)が「実用レベルで使えない」と感じられる根本原因を分析し、
実用化に必要な機能仕様を優先度順に定義する。

> 実装状況: v0.3.0 で P0-1 の `claude-code` / `codex` ビルトイン adapter と、
> P0-2 の `handoff delegate` 同期委譲を実装済み。P1 以降は引き続きロードマップ項目。

## 1. 現状分析:なぜ実用的に使えないか

コードベース(`src/app.rs`、`src/delivery.rs`)と README を踏まえた根本原因は次の4つ。

### 1.1 「エージェントへのハンドオフ」が実際にはエージェントを起動しない

`handoff run reviewer --task ...` は `shell` ランタイムではただのシェルコマンド実行であり、
エージェントに渡すには `HANDOFF_AGENT_CMD_REVIEWER='my-reviewer-agent --task "$HANDOFF_TASK"'`
をユーザーが手で組み立てる必要がある。ツール名が約束する「エージェントへの委譲」が、
インストール直後の状態では成立しない。

### 1.2 受信側が turn ベースの LLM エージェントなので、メッセージが「届かない」

inbox にメッセージは溜まるが、Claude Code 等のエージェントは自分からポーリングしない。

- `mode turn` は Stop フックのため「ターン終了時」しか発火しない。
- `mode monitor` はホストが Monitor ツールを自発的に起動することが前提。

結果として「送ったのに相手が永遠に気づかない」が標準動作になる。
体感的な「使えない」の最大要因。

### 1.3 `actas` による手動の役割切り替えが摩擦そのもの

人間が `handoff actas lead` / `handoff actas reviewer` を切り替えながら使う設計は、
デモはできても日常運用に耐えない。セッション ID(`CLAUDE_CODE_SESSION_ID` 等)からの
自動推定は現状 lease 取得にしか使われていない。

### 1.4 プリミティブの集合であって、ワークフローがない

「diff をレビューさせて結果を自分のコンテキストに戻す」という典型ユースケースに、
`context create` → `to` → `run` → `status` → `result` と5コマンド必要。

## 2. 実用レベルの定義(ターゲットユースケース)

仕様の取捨選択の基準として、「これが1コマンドでできたら実用」というユーザーストーリーを3つ定義する。

| ID | ストーリー |
|----|-----------|
| US1(委譲) | Claude Code セッション内から「この diff を reviewer に見せて結果をもらう」が1コマンド・同期で完結する |
| US2(並行協調) | 2つの生きたエージェントセッションが、相手の応答に数秒以内に気づいて会話を継続できる |
| US3(ゼロ設定) | 新しいプロジェクトで `handoff setup claude-code` 一発で、MCP・フック・identity が全部入る |

## 3. 提案仕様(優先度順)

### P0-1: ビルトイン・エージェントランタイム

`HANDOFF_AGENT_CMD_*` の手組みを不要にし、主要ヘッドレス CLI をファーストクラスのランタイムにする。

```sh
handoff join demo reviewer --runtime claude-code   # 内部で `claude -p` を使う
handoff join demo fixer    --runtime codex          # 内部で `codex exec`
```

`handoff run reviewer --task "..."` 実行時、ランタイムに応じて:

- `claude-code` → `claude -p "$PROMPT" --output-format json` を spawn
- `codex` → `codex exec "$PROMPT" --json`
- タスク本文 + 添付 context を1つのプロンプトに合成して渡す
- stdout の JSON から結果テキストを抽出して `handoff result` に格納
- `--model` / `--allowed-tools` / `--cwd` 等のランタイムオプションは team 設定として永続化
  (`handoff config agent reviewer --set model=...`)

これにより「エージェントにハンドオフする」がツール単体で成立する。

### P0-2: ワンコマンド同期委譲 `handoff delegate`

US1 をそのまま仕様化したファサードコマンド。

```sh
git diff | handoff delegate reviewer --stdin --wait --timeout 300
# → context 作成 + run + ポーリング + result 出力までを1コマンドで
```

- `--wait` で結果を stdout に出し、終了コードをジョブ成否に連動させる
  (呼び出し側エージェントがそのまま結果を読める)
- `--wait` なしなら job-id を返して非同期(現行 `run` 相当)
- `--git-diff` / `--file` / `--stdin` は既存の context オプションを流用

呼び出し側エージェントから見ると「サブエージェント実行」と同じメンタルモデルになり、
MCP ツールとしても `delegate` 1つを覚えれば使える状態になる。

### P1-1: identity の自動化(`actas` の格下げ)

- `handoff join --auto`: セッション環境変数(`CLAUDE_CODE_SESSION_ID` 等)から identity を
  自動生成・自動 bind。以後そのセッションでは `actas` 不要。
- 送信時に active role が未設定なら、セッション ID に bind 済みの role を自動選択。
  曖昧な場合のみエラー。
- `actas` は互換のため残すが、ドキュメント上は「手動オーバーライド」に格下げ。

### P1-2: 配信の信頼性 — `handoff daemon` + 通知ファイル

turn フック頼みをやめ、軽量デーモンでプッシュ配信に寄せる。

- `handoff daemon` がプロジェクト単位で SQLite を watch し、新着メッセージを
  `.handoff/notify/<agent>.md` に書き出す。
- Claude Code 向けは `UserPromptSubmit` / `Stop` 両フックで notify ファイルの有無をチェックし、
  あれば additionalContext として注入(現行の Stop のみより発火機会が増える)。
- `handoff monitor` は現行どおり残すが、daemon が起動を肩代わりする
  (`mode monitor` 時に daemon がストリームを供給)。
- 配信保証として message に `delivered_at` / `read_at` を記録し、
  `handoff to --ack-timeout 60` で「相手が読まなかったら送信側にエラーを返す」を可能にする。

### P2-1: セットアップ一発化 `handoff setup`

```sh
handoff setup claude-code
```

これ1つで以下を実行する:

- `init` + `join --auto`
- MCP サーバー登録(`.mcp.json` への追記)
- `mode both` 相当のフック設置
- 使い方を書いた skill(`.claude/skills/handoff/SKILL.md`)の配置

エージェント自身が「いつ handoff を使うべきか」を知らない問題は skill の配布でしか
解決しないため、これをツールの責務に含める。

### P2-2: 観測性とスレッド管理

- `handoff threads`: スレッド一覧と状態(open / waiting-reply / closed)
- ジョブの dead-letter: timeout / 失敗ジョブを `handoff status --failed` で一覧、
  `retry` に `--with-context` を追加
- `handoff doctor`: フック・MCP・daemon・lease の健全性診断
  (「なぜ届かないのか」を自己診断できるようにする)

## 4. リリース計画

| バージョン | 内容 | 達成されるストーリー |
|-----------|------|---------------------|
| v0.3 | P0-1 ランタイムアダプタ + P0-2 `delegate` | US1: 1コマンド委譲 |
| v0.4 | P1-1 auto identity + P1-2 daemon 配信 | US2: 双方向のリアルタイム協調 |
| v0.5 | P2-1 `setup` + skill 配布 + P2-2 `doctor` | US3: ゼロ設定 |

## 5. 設計判断のポイント

最も効くのは **P0-1 + P0-2**。現状の「メッセージング基盤」路線は、受信側がポーリングしない
LLM エージェントである以上、単体では実用にならない。一方「ヘッドレス CLI を spawn する同期委譲」は
今日の `claude -p` / `codex exec` で確実に動き、既存の context / job / SQLite 基盤を
そのまま活かせる。

メッセージングは「生きたセッション同士の協調(US2)」用の機能として、daemon 配信とセットで
後段(v0.4)に回すのが現実的である。

## 6. v0.5 以降の拡張ロードマップ(v0.6〜v1.0)

v0.5 で3つのユーザーストーリー(委譲・並行協調・ゼロ設定)が達成された前提に立つと、
その先は「単発の委譲が動く」段階から「複数エージェントの運用基盤」へ進む段階になる。

### v0.6 — マルチエージェント・ワークフロー(オーケストレーション)

v0.5 までは1対1の委譲が単位。次は委譲の「合成」を仕様化する。

```sh
handoff flow run review-pipeline.yml --input "$(git diff)"
```

- YAML でパイプラインを宣言: `plan → implement → review → fix` の直列、
  レビュアー3並列 fan-out → 集約(judge)などの DAG
- 各ステップは既存の `delegate` をそのまま使う(ジョブ基盤・SQLite はそのまま活きる)
- ステップ間のデータ受け渡しは context package を自動チェーン
- `handoff flow status` で DAG の進行を可視化

### v0.7 — クロスマシン/リモート連携

local-first の設計は維持しつつ、トランスポートを抽象化する。

- `handoff remote add buildserver ssh://...`: SSH 経由で別マシンの handoff に
  メッセージ・ジョブを転送
- 用途: 重いビルド/テストはリモートの worker エージェントへ、レビューはローカルで
- アカウント・サーバー不要の方針は守る(SSH と既存の SQLite だけで成立)

### v0.8 — セキュリティとガバナンス

README 自身が「sandbox ではない」と明記している点を解消する段階。

- エージェントごとの権限プロファイル: `handoff config agent reviewer --allow read-only`
  (claude-code ランタイムなら `--allowedTools` に変換して強制)
- 監査ログ: 誰がいつ何を委譲し、何が実行されたかを `handoff audit` で追跡
- `--cmd` / adapter コマンドの allowlist 化

### v0.9 — 観測性とコスト管理

- `handoff top`: 実行中ジョブ・メッセージフローのライブ TUI ダッシュボード
- トークン/コスト集計: ランタイムアダプタが取り込む usage 情報をもとに
  `handoff cost --by-agent` で集計
- メトリクスのエクスポート(JSON Lines)

### v1.0 — 安定化と拡張 API

- ストレージスキーマと CLI の互換性保証(semver 凍結)、スキーママイグレーションの自動化
- カスタムランタイムのプラグイン API
  (任意のエージェント CLI を adapter として登録する公式手順)
- ロールテンプレート配布: `handoff join demo reviewer --template security-auditor` のように
  プロンプト+権限+モデル設定をプリセット化

### 拡張ロードマップの優先順位

v0.6(ワークフロー)が最も価値が高い。1対1の delegate が動いた瞬間に、ユーザーが
次にやりたくなるのは「並列レビュー」「plan → implement → review の自動連鎖」であり、
これは既存のジョブ・context 基盤の合成だけで実現でき、新しいインフラがほぼ不要だからである。

逆に v0.7(リモート)はトランスポート抽象化という大きな設計変更を伴うため、
需要を見てから着手するのが安全である。
