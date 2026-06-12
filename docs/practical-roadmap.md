# agent-handoff 実用化仕様(v0.3〜v0.5 ロードマップ)

本ドキュメントは、現状の MVP(v0.2.0 時点)が「実用レベルで使えない」と感じられる根本原因を分析し、
実用化に必要な機能仕様を優先度順に定義する。

> 実装状況: v0.3.0 で P0-1/P0-2 を実装済み。この変更で P1-1 の
> profile/session 分離、P1-2 の daemon/notify 配信、P2-1 の `handoff setup`
> の実用スライスを実装した。旧 `join` / `actas` / `drop` / `rename-team` は
> 公開 CLI から削除済み。

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

### 1.3 「役割(role)」概念そのものが過剰

`handoff join demo reviewer` で作られる role は、性質の違う3つの機能を1つの概念に
詰め込んでいる。

1. **実行プロファイル** — reviewer をどのランタイム・モデル・プロンプト・権限で動かすか
2. **宛先(アドレス)** — メッセージをどの inbox に届けるか
3. **送信者の人格切り替え** — `actas` で「いま自分は誰か」を選ぶ

このうち 3 は純粋なコストで、価値を生んでいない。人間が `handoff actas lead` /
`handoff actas reviewer` を切り替えながら使う設計は、デモはできても日常運用に耐えない。
摩擦の根本原因は actas の操作コストではなく、**そもそも送信者に名前を付けて
切り替えさせる必要がない**ことにある。

team / join / inbox という語彙は agmsg 由来で、複数の「人」が協調するメタファーだが、
LLM エージェントでは役割の演じ分けはアドレッシング層ではなく**プロンプトの仕事**である。
「reviewer らしさ」はプロファイルのシステムプロンプトに書けばよく、メッセージ基盤が
役割を知っている必要はない。

この分析に基づき、本仕様では role を **profile(委譲用の実行テンプレート)** と
**session(対話用の宛先)** に分離・解体する(P1-1)。

### 1.4 プリミティブの集合であって、ワークフローがない

「diff をレビューさせて結果を自分のコンテキストに戻す」という典型ユースケースに、
`context create` → `to` → `run` → `status` → `result` と5コマンド必要。

## 2. 実用レベルの定義(ターゲットユースケース)

仕様の取捨選択の基準として、「これが1コマンドでできたら実用」というユーザーストーリーを3つ定義する。

| ID | ストーリー |
|----|-----------|
| US1(委譲) | Claude Code セッション内から「この diff を reviewer に見せて結果をもらう」が1コマンド・同期で完結する |
| US2(並行協調) | 2つの生きたエージェントセッションが、相手の応答に数秒以内に気づいて会話を継続できる |
| US3(ゼロ設定) | 新しいプロジェクトで `handoff setup claude-code` 一発で、MCP・フック・セッション登録が全部入る |

## 3. 提案仕様(優先度順)

### P0-1: ビルトイン・エージェントランタイム

`HANDOFF_AGENT_CMD_*` の手組みを不要にし、主要ヘッドレス CLI をファーストクラスのランタイムにする。

エージェントの起動方法は identity ではなく **profile**(inbox も lease も持たない
実行テンプレート)として定義する。

```sh
handoff profile create reviewer --runtime claude-code --prompt-file reviewer.md
handoff profile create fixer    --runtime codex
```

`handoff run reviewer --task "..."` 実行時、profile のランタイムに応じて:

- `claude-code` → `claude -p "$PROMPT" --output-format json` を spawn
- `codex` → `codex exec "$PROMPT" --json`
- タスク本文 + 添付 context + profile のシステムプロンプトを1つのプロンプトに合成して渡す
- stdout の JSON から結果テキストを抽出して `handoff result` に格納
- `--model` / `--allowed-tools` / `--cwd` 等のランタイムオプションは profile に永続化
  (`handoff profile set reviewer model=...`)

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

### P1-1: role 概念の解体 — profile と session への分離

1.3 の分析に基づき、role を2つの直交する概念に置き換える。

- **profile(委譲用)**: P0-1 で定義した実行テンプレート。inbox も lease も持たない。
- **session(対話用)**: 生きたエージェントセッションそのものが宛先。起動時に
  セッション環境変数(`CLAUDE_CODE_SESSION_ID` 等)から自動登録され、`handoff sessions`
  で一覧できる。名前は人間が読みやすくするための**任意のエイリアス**に過ぎない。
  host 固有のセッション ID が無い MCP サーバーや通常のターミナルでは、
  `handoff` がプロジェクトローカルな fallback session ID を生成して永続化し、
  以後の CLI/MCP 呼び出しで同じ「いまのセッション」として再利用する。
  `HANDOFF_SESSION_ID` が明示されている場合はそれを最優先し、MCP wrapper は必要に応じて
  生成済み fallback ID を子プロセス環境へ渡せるようにする。

CLI の変更:

- `join` / `actas` / `drop` / `rename-team` を廃止(deprecation 期間後に削除)
- 送信者は常に「いまのセッション」で自動決定。曖昧さは原理的に発生しない
- 宛先はライブセッションを指定。読みやすいエイリアスは `@alias` として送る:
  `handoff to @<alias> <message>`

トレードオフ: セッションをまたぐ永続的な宛先(「reviewer 宛に送っておけば、次に
reviewer セッションが立ち上がった時に読む」)は失われる。このユースケースが実際に
必要になった場合は「エイリアスに inbox を持たせる」形で後付けできるため、
先回りして概念を増やすことはしない。

学習コストの効果: 覚える概念が「team / agent / actas / lease」の4つから
「profile(委譲用)/ session(対話用)」の2つに減る。

### P1-2: 配信の信頼性 — `handoff daemon` + 通知ファイル

turn フック頼みをやめ、軽量デーモンでプッシュ配信に寄せる。

- `handoff daemon` がプロジェクト単位で SQLite を watch し、新着メッセージを
  `.handoff/notify/<session>.md` に書き出す。
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

- `init` + セッションの自動登録(P1-1 の session 概念。join 操作は存在しない)
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
| v0.4 | P1-1 role 解体(profile / session 分離)+ P1-2 daemon 配信 | US2: 双方向のリアルタイム協調 |
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

- profile ごとの権限設定: `handoff profile set reviewer allow=read-only`
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
- profile テンプレート配布: `handoff profile install security-auditor` のように
  プロンプト+権限+モデル設定をプリセットとして取り込み可能にする

### 拡張ロードマップの優先順位

v0.6(ワークフロー)が最も価値が高い。1対1の delegate が動いた瞬間に、ユーザーが
次にやりたくなるのは「並列レビュー」「plan → implement → review の自動連鎖」であり、
これは既存のジョブ・context 基盤の合成だけで実現でき、新しいインフラがほぼ不要だからである。

逆に v0.7(リモート)はトランスポート抽象化という大きな設計変更を伴うため、
需要を見てから着手するのが安全である。
