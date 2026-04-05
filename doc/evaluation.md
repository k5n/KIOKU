# LoCoMo / LongMemEval の評価方法

## 1. この評価で何を測るのか

記憶層ベンチマークでは、次の 3 つを分けて考える必要があります。

1. 記憶の追加: 会話履歴を時系列で受け取り、内部状態に取り込めるか
2. 記憶の検索: 質問に対して、必要な記憶を返せるか
3. 記憶を使った回答: 返された記憶を使って、最終回答を正しく作れるか

`EverOS/evaluation` が採っている `Add -> Search -> Answer -> Evaluate` の 4 段構成は、この分離をそのまま実装したものになっており、`kioku` でも同じ分離を採るのがよいです。

特に重要なのは、**記憶層そのもの** と **回答生成 LLM** と **採点器** を分離することです。  
比較対象は「どの記憶を返したか」であるべきなので、回答生成と採点はバックエンド間で固定する必要があります。

ここでいう「バックエンド共通 I/F」は、KIOKU 本体のドメイン I/F を意味しません。  
評価プログラムが `Add -> Search -> Answer -> Evaluate` を回すために必要な `MemoryBackend` は、`crates/evaluate` 側が持つ評価用 I/F と考えるのが自然です。

`crates/core` は KIOKU というシステム自体のドメイン層、`crates/adapters/*` はそのインフラ層です。  
将来 KIOKU を評価に載せるときは、`crates/evaluate` 側に `KiokuMemoryBackend` を実装して接続します。

## 2. 共通の評価フロー

LoCoMo と LongMemEval はデータ構造が異なりますが、評価手順自体はかなり共通化できます。

1. データセットを共通形式に正規化する
2. ケース単位で記憶層を初期化する
3. 会話メッセージを時系列順に `ingest` する
4. 質問時点で `query` を呼び、記憶層が返したコンテキストを得る
5. 固定の Answerer に `question + retrieved memories` を渡して回答させる
6. 固定の Judge で gold answer と照合する
7. retrieval 指標と answer 指標を別々に集計する

この流れであれば、最初は「全メッセージをそのまま返すスタブ」で動かし、その後に本物の記憶層へ差し替えられます。

### 2.1 先に用意すべきベースライン

実装初期に最低限必要なのは次の 3 本です。

- `return-all`: 追加済みメッセージを全部返す。今回のスタブ実装そのもの
- `full-context`: Answerer に会話全体をそのまま渡す。記憶検索を通さない上限ベースライン
- `oracle`: gold evidence のみを渡す。Judge と Answerer の健全性確認用

`return-all` と `full-context` は似ていますが、前者は「記憶層 I/F を通した実装確認」、後者は「検索を介さない理想上限」です。  
最初の段階ではこの 2 つがほぼ同値になるはずで、差が出たら runner 側にバグがある可能性が高いです。

## 3. LoCoMo の評価方法

### 3.1 評価単位

LoCoMo では **1 サンプル = 1 本の長い会話** です。  
その中に複数セッションと複数 QA が入っています。

したがって評価単位は次のようになります。

- 記憶層の初期化単位: 1 conversation sample
- `ingest` の単位: その sample 内の全発話
- `query` の単位: sample 内の各 QA

つまり、**1 サンプルにつき記憶を 1 回構築し、その上で複数質問を投げる** のが基本です。  
これは `MAGMA/test_fixed_memory.py` の流れと一致します。

### 3.2 ingest の流れ

会話はセッション単位で順序を持っているので、次の順で追加すれば十分です。

1. `session_1`, `session_2`, ... の順に処理する
2. 各セッション内ではメッセージ配列順に処理する
3. セッション時刻を基準に、turn ごとの疑似 timestamp を与える

LoCoMo にはセッション時刻はありますが、通常は turn 単位の timestamp はありません。  
そのため `EverOS/evaluation/src/core/loaders.py` のように、**セッション時刻から turn ごとの擬似 timestamp を決め打ちで振る** のが実用的です。

`kioku` でも次のような決定的ルールで十分です。

- セッション開始時刻はデータに入っている `session_X_date_time` を使う
- 同一セッション内は turn 順に `+30 sec` ずつ振る
- 次セッション開始時刻に収まらない場合だけ interval を圧縮する

この timestamp は厳密な正解ではなく、**バックグラウンド処理を進める順序制御用の疑似時刻** と考えるべきです。

### 3.3 query の流れ

LoCoMo の QA は、会話全体を読んだ後に解く想定です。  
したがって各 QA については、**そのサンプルの全会話を ingest し終えた後** に `query` を呼ぶのが自然です。

query 入力は最低限次を持てば足ります。

- conversation / room を識別する ID
- 質問文
- 質問時刻
- 取得件数や granularity などの検索パラメータ

最初のスタブ実装では、質問文を無視して全メッセージを返して構いません。

### 3.4 answer の採点

### 3.4.1 主指標

LoCoMo は自由記述の短答が多く、表記揺れや日付表現の差があるため、**主指標は LLM Judge による正誤判定** にするのがよいです。

参考実装を見ると、次の傾向があります。

- `EverOS/evaluation`:
  - LLM Judge を 3 回回し、平均 accuracy と標準偏差を出す
  - カテゴリ別 accuracy も出す
- `MAGMA/test_fixed_memory.py`:
  - Exact Match / F1 / BLEU-1 / LLM Judge を併記する
  - 集計の主眼は accuracy とカテゴリ別 breakdown にある

`kioku` でも次の方針が妥当です。

- ヘッドライン指標: LLM Judge accuracy
- 補助指標: Exact Match / token F1 / BLEU-1

補助指標はデバッグには有用ですが、最終比較指標としては LLM Judge の方が妥当です。

### 3.4.2 カテゴリ

`MAGMA/test_fixed_memory.py` で使われているカテゴリ名は次です。

- `1`: Multi-hop
- `2`: Temporal
- `3`: Open-domain
- `4`: Single-hop
- `5`: Adversarial

`crates/evaluate/src/bin/locomo.rs` ではまだカテゴリ名が明示されていませんが、評価レポートではこの名前で出すのが分かりやすいです。

### 3.4.3 category 5 の扱い

LoCoMo の `category = 5` は adversarial question です。  
この場合の gold は通常の `answer` ではなく `adversarial_answer` を見る必要があります。`MAGMA/load_dataset.py` もその扱いをしています。

ただし、既存の比較実装では main score から除外されることが多いです。

- `EverOS/evaluation/config/datasets/locomo.yaml` は既定で category 5 を filter out している
- `MAGMA/test_fixed_memory.py` もデフォルトでは `1,2,3,4` を評価対象にしている

したがって `kioku` でも次の 2 本立てにするのがよいです。

- 主要スコア: category 1-4
- 参考スコア: category 5 を別枠で表示

### 3.5 LoCoMo で取るべき retrieval 指標

LoCoMo の JSON には各 QA に `evidence` があり、これは正解の根拠 turn の `dia_id` 群です。  
この情報を使うと、最終回答だけでなく retrieval 自体も評価できます。

LoCoMo 公式の標準 retrieval script が手元参照にはないため、ここは **本リポジトリで追加する提案指標** です。

少なくとも次を取る価値があります。

- turn-level `hit_any@k`: top-k に evidence turn が 1 つでも入ったか
- turn-level `recall_all@k`: top-k に evidence turn がすべて入ったか
- turn-level `mrr`: 最初の evidence turn が何位に来たか
- session-level `hit_any@k`: evidence turn を含むセッションが top-k に入ったか

LoCoMo は multi-hop / temporal のように複数 turn を跨いで答える質問があるため、`hit_any@k` だけだと甘いです。  
最低でも `recall_all@k` か `evidence_coverage` を併記した方がよいです。

### 3.6 LoCoMo のレポート形式

LoCoMo では次を最低限出せば十分です。

- overall accuracy (`category 1-4`)
- per-category accuracy (`1-4`)
- optional adversarial accuracy (`category 5`)
- retrieval metrics (`hit_any@k`, `recall_all@k`, `mrr`)
- 取得コンテキスト量:
  - 返したメッセージ数
  - Answerer に渡した token 数
- 実行コスト:
  - ingest latency
  - query latency

## 4. LongMemEval の評価方法

### 4.1 評価単位

LongMemEval では **1 entry = 1 問** です。  
各 entry の中に、その質問に対応する `haystack_sessions` が埋め込まれています。

LoCoMo との最大の違いはここで、LongMemEval は **質問ごとに独立した記憶状態を再構築する** のが基本です。

- 記憶層の初期化単位: 1 question entry
- `ingest` の単位: その entry の `haystack_sessions`
- `query` の単位: その entry の `question`

この構造は `MAGMA/test_longmemeval_chunked.py` の実装とも一致しています。

### 4.2 ingest の流れ

LongMemEval では各 entry に次が入っています。

- `haystack_session_ids`
- `haystack_dates`
- `haystack_sessions`
- `question_date`

したがって ingest は次の順で行えば十分です。

1. `haystack_dates` の昇順で session を処理する
2. 各 session 内では turn 配列順に処理する
3. turn timestamp は session timestamp から疑似生成する
4. 全 session を入れ終わった時点、または `question_date` で `query` を呼ぶ

公式 README では、`longmemeval_s_cleaned.json` と `longmemeval_m_cleaned.json` の `haystack_session_ids` は timestamp 順に並んでいると明記されています。  
ただし `longmemeval_oracle.json` は必ずしも sort 済みではないので、runner 側で日付順に sort しておくと安全です。

### 4.3 LongMemEval の正式な answer 評価

LongMemEval の QA 評価は、LoCoMo のような単一 judge prompt ではなく、**question type ごとに rubric が異なる** のが重要です。  
これは公式 `evaluate_qa.py` と `MAGMA/memory/longmemeval_evaluator.py` が共通して採っている方針です。

### 4.3.1 question type

公式 README にある question type は次です。

- `single-session-user`
- `single-session-assistant`
- `single-session-preference`
- `temporal-reasoning`
- `knowledge-update`
- `multi-session`

加えて、**`question_id` が `_abs` で終わるものは abstention question** です。  
これは `question_type` とは別軸です。

### 4.3.2 type ごとの採点ルール

| type | 採点ルール |
| --- | --- |
| `single-session-user` / `single-session-assistant` / `multi-session` | 回答が gold を含むか。必要情報の一部だけでは不正解 |
| `temporal-reasoning` | 日数や週数などの数え上げで off-by-one を許容する |
| `knowledge-update` | 古い情報を含んでいても、最新の更新後の答えが合っていれば正解 |
| `single-session-preference` | rubric の全項目を満たす必要はなく、ユーザ情報を正しく想起して personalization できていれば正解 |
| abstention (`question_id` ends with `_abs`) | 「答えられない」「情報が足りない」と正しく判断できたら正解 |

したがって、LongMemEval を LoCoMo と同じ generic judge で採点すると、特に `temporal-reasoning` と `knowledge-update` の妥当性が落ちます。

`EverOS/evaluation` はアーキテクチャ参考としては有用ですが、LongMemEval の採点については **公式 `evaluate_qa.py` に寄せる方がよい** です。

### 4.4 LongMemEval の集計方法

公式 `print_qa_metrics.py` は次を出します。

- task-averaged accuracy
- overall accuracy
- abstention accuracy
- question type ごとの accuracy

このうち `task-averaged accuracy` は、各 type の accuracy を単純平均したものです。  
type のデータ数に偏りがある場合でも、能力ごとのバランスを見やすいので、`kioku` でも採用した方がよいです。

### 4.5 LongMemEval の retrieval 指標

LongMemEval には retrieval 用の gold も入っています。

- `answer_session_ids`: 正解 evidence を含む session の ID 群
- `haystack_sessions[*][*].has_answer = true`: 正解 evidence を含む turn

公式 README と `print_retrieval_metrics.py` から、少なくとも次の指標を出すことが分かります。

- session-level:
  - `recall_all@5`
  - `ndcg_any@5`
  - `recall_all@10`
  - `ndcg_any@10`
- turn-level:
  - `recall_all@5`
  - `ndcg_any@5`
  - `recall_all@10`
  - `ndcg_any@10`
  - `recall_all@50`
  - `ndcg_any@50`

ここでの意味は次のように理解してよいです。

- `recall_all@k`: 正解 evidence を top-k にすべて含められたか
- `ndcg_any@k`: 正解 evidence のどれかを、より上位に置けているほど高い指標

なお、公式 README は **retrieval 評価では 30 個の abstention instance を常に除外する** と明記しています。  
`kioku` でも同じ扱いにするのが自然です。

補足: abstention instance とは「答えられない」「情報が足りない」と正しく判断できたかを問う質問です。retrieval ではそもそも正解 evidence が存在しないので、評価から除外されます。

### 4.6 現在の `crates/evaluate` ローダーとの関係

現状の `crates/evaluate/src/bin/longmemeval.rs` は、`has_answer` を保持していません。  
しかし公式 LongMemEval では turn-level retrieval 評価に `has_answer` を使います。

したがって今後は次の修正が必要です。

- `LongMemEvalMessage` に `has_answer: Option<bool>` を追加する
- `answer_session_ids` だけでなく `has_answer` も保持する
- abstention を `question_id.ends_with("_abs")` で判定できるようにする

session-level 指標だけなら現状の型でも計算できますが、turn-level 指標まで正しく出すにはこの修正が必要です。

### 4.7 LongMemEval のレポート形式

LongMemEval では次を最低限出すべきです。

- overall accuracy
- task-averaged accuracy
- per-type accuracy
- abstention accuracy
- session-level retrieval metrics
- turn-level retrieval metrics
- Answerer に渡した session 数 / token 数
- query latency

## 5. `kioku` で採るべき最終方針

上記を踏まえると、`kioku` の評価方針は次がよいです。

1. 評価パイプラインは `Add -> Search -> Answer -> Evaluate` に分ける
2. 最初は `return-all` スタブで LoCoMo / LongMemEval の両方を通す
3. LoCoMo は main score を `category 1-4` に置き、`category 5` は別枠にする
4. LongMemEval は official の type-specific judge を使う
5. retrieval 指標と answer 指標を必ず分離して保存する
6. まず full-context ベースラインを作り、その後に本物の記憶検索へ差し替える

この形にしておけば、将来 `crates/adapters/*` にどんな記憶実装を足しても、**同じ入力イベント列を食わせて、同じ Judge で比較** できます。

## 6. 参考にした実装と資料

- `EverOS/evaluation/README.md`
- `EverOS/evaluation/src/core/pipeline.py`
- `EverOS/evaluation/src/evaluators/llm_judge.py`
- `EverOS/evaluation/src/converters/longmemeval_converter.py`
- `../magma-rs/MAGMA/test_fixed_memory.py`
- `../magma-rs/MAGMA/test_longmemeval_chunked.py`
- `../magma-rs/MAGMA/memory/evaluator.py`
- `../magma-rs/MAGMA/memory/longmemeval_evaluator.py`
- LongMemEval 公式 README: <https://github.com/xiaowu0162/LongMemEval>
- LongMemEval 公式評価 script:
  - <https://github.com/xiaowu0162/LongMemEval/blob/main/src/evaluation/evaluate_qa.py>
  - <https://github.com/xiaowu0162/LongMemEval/blob/main/src/evaluation/print_qa_metrics.py>
  - <https://github.com/xiaowu0162/LongMemEval/blob/main/src/evaluation/print_retrieval_metrics.py>
