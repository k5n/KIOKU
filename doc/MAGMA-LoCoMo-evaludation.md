# MAGMA の LoCoMo 評価実装メモ

## 1. 目的

この文書は、`./MAGMA/test_fixed_memory.py` を起点に、MAGMA が LoCoMo をどう評価しているかを整理するためのメモです。  
特に次の 2 点を分けて記述します。

1. MAGMA が実際に何を実行しているか
2. `doc/LoCoMo-evaluation.md` に整理した LoCoMo 公式実装と何が違うか

ここでいう「MAGMA の LoCoMo 評価」は、主に次のコードを指します。

- `MAGMA/test_fixed_memory.py`
- `MAGMA/memory/test_harness.py`
- `MAGMA/memory/evaluator.py`
- `MAGMA/memory/llm_judge.py`
- `MAGMA/memory/answer_formatter.py`
- `MAGMA/utils/utils.py`
- `MAGMA/load_dataset.py`

## 2. 結論

先に要点だけ書くと、MAGMA の LoCoMo 評価は official とかなり違います。

- official は per-question の deterministic な F1 judge を中心に集計する
- MAGMA は `Exact Match / F1 / BLEU-1 / LLM judge` を併記する
- MAGMA の `Accuracy` は LLM judge ではなく `exact_match` ベースである
- MAGMA は既定で `category 1-4` のみを対象にし、`category 5` を外す
- MAGMA は既定で `sample 0`、`max_questions=50`、`best-of-3` で走る
- しかも `best-of-3` の選抜に gold answer と LLM judge を使っており、official にはない gold leakage が入っている
- official の category 別採点分岐
  - category 1 の comma-split multi-answer F1
  - category 3 の `;` 前半だけを使う処理
  - category 5 の official 固有実装
  を MAGMA は再現していない

したがって MAGMA の LoCoMo 評価は、official の忠実再現ではなく、MAGMA 独自の QA evaluation pipeline と見るのが正確です。

## 3. 実行単位と既定設定

`MAGMA/test_fixed_memory.py` の CLI 既定値は次です。

- `--dataset data/locomo10.json`
- `--sample [0]`
- `--max-questions 50`
- `--category-to-test "1,2,3,4"`
- `--best-of-n 3`
- `--best-of-n-method llm_judge`
- parallel 実行有効

つまり、**何も指定せず実行した場合は LoCoMo 全体を評価しません**。  
既定では次の subset evaluation です。

1. sample 0 のみ
2. category 1-4 のみ
3. フィルタ後の先頭 50 問まで
4. 各問を 3 回答えさせて best-of-3 選抜

これは official の「全 QA を採点して category 別 / overall を出す」評価とはかなり違います。

## 4. データ読み込みと前処理

### 4.1 QA の gold answer

`MAGMA/load_dataset.py` では、QA の gold answer は `QA.final_answer` で取得されます。

- category 1-4: `answer`
- category 5: `adversarial_answer`

したがって、MAGMA は category 5 のとき `answer` ではなく `adversarial_answer` を gold として保持します。

ただし後述の通り、実際の採点では category 5 の gold 文字列はほぼ使われません。

### 4.2 画像付き turn の扱い

`parse_session()` は画像付き turn に対して `blip_caption` を本文へ埋め込みます。

- 形式: `[Image: ...] text`

したがって、MAGMA の記憶構築対象は LoCoMo JSON の素の text そのものではなく、caption を合成した turn テキストです。

## 5. 1 sample あたりの評価フロー

`test_fixed_memory.py` の main loop は、概ね次の流れです。

1. dataset をロードする
2. 指定 sample を 1 つ選ぶ
3. MemoryBuilder で sample 全体の memory graph を構築または cache からロードする
4. `sample.qa` を category で filter する
5. `max_questions` 件まで question を流す
6. 各 question について retrieve -> answer -> evaluate を行う
7. sample 単位で metrics を集計する
8. 複数 sample の場合は sample 平均をさらに出す

評価単位そのものは official と同じく「1 sample = 1 conversation、sample 内に複数 QA」です。  
ただし default 実行条件が subset なので、**実装上は sample 単位でも、運用上は部分評価になりやすい**です。

## 6. retrieve の条件

`MAGMA/memory/test_harness.py` では question ごとに `query_engine.query()` を呼びます。  
retrieve 件数は category ごとに固定されています。

- category 1 (Multi-hop): `top_k = 30`
- それ以外: `top_k = 15`

official の公開 QA judge は retrieval 条件をこのように category ごとに固定していません。  
この `top_k` は MAGMA 独自の実験条件です。

また、MAGMA は LoCoMo の `evidence` を使った official 風の retrieval recall 集計を行っていません。  
保存しているのは主に次です。

- `context_nodes`
- `search_details`
- 低スコア時の `top_nodes`

つまり、**official が持つ answer 側 F1 と retrieval 側 recall の 2 軸集計は、`test_fixed_memory.py` では再現されていません**。

## 7. answer 生成の仕方

### 7.1 category 別 prompt

`MAGMA/memory/answer_formatter.py` は、category ごとにかなり違う prompt を使います。

- category 1:
  - `KEY FACTS` を見て facts を connect するよう指示
  - list / count / yes-no の出力形式を指定
- category 2:
  - 相対日付から date を計算するよう指示
  - 出力形式を `D Month YYYY` に寄せる
- category 3:
  - reasonable inference を許可する
  - `Yes/No, because ...` や personality trait を出させる
- category 4:
  - specific fact を短く抽出するよう指示
- category 5:
  - entity mismatch を厳密に検出し、怪しければ `Information not found`

official 実装は category 1-4 で「context に基づいて短いフレーズで答える」「可能なら会話内の正確な語を使う」を基本にしており、MAGMA の方が prompt engineering がかなり強いです。  
特に category 3 は official よりも明確に inference を促しています。

### 7.2 category 5 の answer 生成

official の category 5 は、質問を二択形式へ変換して

- `(a) Not mentioned in the conversation`
- `(b) 候補 answer`

のどちらかを選ばせる形式でした。

一方 MAGMA は二択化をしていません。  
category 5 でも通常の free-form answer を生成し、その後で `validate_adversarial_answer()` によって

- 質問 entity と answer entity が合っているか
- 不自然に具体的な hallucination になっていないか

を見て、必要なら `Information not found` に補正します。

したがって category 5 の answer generation は official とかなり違います。

## 8. best-of-N 選抜

MAGMA の最大の差分の 1 つがここです。

### 8.1 既定で best-of-3

`test_fixed_memory.py` の default は `--best-of-n 3` です。  
そのため MAGMA は各 question を 1 回ではなく 3 回回答します。

### 8.2 既定で LLM judge による選抜

さらに default の `--best-of-n-method` は `llm_judge` です。  
`TestHarness._answer_question_best_of_n()` は各試行 answer に対して evaluator を呼び、**gold answer を使って** best answer を選びます。

つまり既定評価では、

1. 同じ question に対して 3 回回答を生成する
2. 各回答を gold answer つきで採点する
3. 最も高得点の answer を最終 answer として採用する

という流れになっています。

これは official LoCoMo にはない処理です。  
しかも **gold answer を answer selection に使っている** ので、純粋な inference evaluation ではありません。

実質的には「推論 + oracle 的 reranking」が入っています。

## 9. judge 実装

### 9.1 evaluator の返り値

`MAGMA/memory/evaluator.py` の `evaluate_answer()` は、1 問あたり次を返します。

- `metrics`
- `is_correct`
- `llm_judge_score`
- `llm_judge_reasoning`

ここで重要なのは `is_correct` の意味です。

### 9.2 `correct` は exact match

`Evaluator.evaluate_answer()` は `is_correct = metrics["exact_match"]` としています。  
`test_fixed_memory.py` の `Correct` / `Accuracy` はこの `correct` をそのまま合計しているため、**MAGMA の Accuracy は exact-match accuracy です**。

つまり MAGMA の表記上:

- `Accuracy`: exact match の比率
- `Average F1`: token overlap F1 の平均
- `Average BLEU-1`: BLEU-1 平均
- `Average LLM Judge Score`: 0.0-1.0 の連続値平均

であり、headline の `Accuracy` は LLM judge ではありません。

### 9.3 non-category-5 の metric

`MAGMA/utils/utils.py` の `calculate_metrics()` は、category 5 以外では次のように採点します。

- `exact_match`
  - `prediction.lower() == reference.lower()`
  - strip はするが、official のような punctuation/article/and 除去はしない
- `f1`
  - `simple_tokenize()` で tokenize
  - token の **set** を作って overlap F1
  - stemming なし
  - token multiplicity は無視
- 追加で
  - `ROUGE`
  - `BLEU`
  - `BERTScore`
  - `METEOR`
  - `Sentence-BERT similarity`

ただし `test_fixed_memory.py` が集計して前面に出すのは主に

- Accuracy
- F1
- BLEU-1
- LLM Judge

です。

### 9.4 official との judge 差分

official と比べると、MAGMA の judge は次の点で違います。

1. official は deterministic token-level F1 が主で、LLM judge を使わない
2. official は normalization が強い
   - lower
   - punctuation 除去
   - `a/an/the` 除去
   - `and` 除去
   - stemming
3. official は category 1 を comma-split multi-answer F1 として特別扱いする
4. official は category 3 で `answer.split(';')[0]` を使う special case がある
5. MAGMA は category 1-4 を基本的に同じ metric 関数で採点する

したがって MAGMA の `F1` は official の `F1` と同じ指標名でも中身が違います。

## 10. category 5 の採点

### 10.1 実際の判定条件

MAGMA の category 5 は、`calculate_metrics()` でも `LLMJudge.evaluate_answer()` でも、

- answer が unanswerable 系表現なら正解
- 具体的 answer を返したら不正解

という 0/1 判定です。

このとき gold answer として `adversarial_answer` は保持されていますが、採点では実質使いません。  
見るのは prediction が

- `not mentioned`
- `information not found`
- `unknown`
- `no information`

などに該当するかどうかです。

### 10.2 official との差分

official でも category 5 は特殊採点ですが、MAGMA とは次が違います。

- official は answer generation を二択問題へ変換している
- MAGMA は free-form answer を出してから unanswerable 判定する
- MAGMA は entity mismatch 検出で `Information not found` に強制補正する

そのため category 5 の score は、official の公開フローそのままではありません。

## 11. LLM judge の位置づけ

### 11.1 continuous score

`MAGMA/memory/llm_judge.py` は `gpt-4o-mini` を hardcode して使い、0.0 から 1.0 の連続スコアを返します。  
部分点ありの semantic judge です。

### 11.2 何に使われるか

この LLM judge は少なくとも次の 3 箇所で使われます。

1. answer quality の補助指標
2. best-of-N の answer selection
3. `llm_score < 0.5` のとき wrong case とみなして詳細 context を保存

つまり MAGMA における LLM judge は、単なる参考表示ではなく、**最終 answer の選抜にも debug 出力にも影響する中心部品**です。

### 11.3 official との差分

official LoCoMo QA evaluation は LLM judge ベースではありません。  
したがって MAGMA の `Average LLM Judge Score` は official 互換指標ではありません。

## 12. 集計方法

### 12.1 sample 内集計

`test_fixed_memory.py` は sample ごとに次を出します。

- All Categories
  - `Total`
  - `Correct`
  - `Accuracy`
  - `Average F1`
  - `Average BLEU-1`
  - `Average LLM Judge Score`
  - `Information not found`
- WITHOUT Category 5
- BY CATEGORY

既定では category 5 を最初から filter しているので、default 実行時の `All Categories` は実質 `1-4` です。

### 12.2 複数 sample 集計

複数 sample を指定したときの aggregate は、QA を全部プールして平均するのではなく、**sample ごとの指標を単純平均**しています。

例えば overall accuracy は

- 各 sample の accuracy を先に計算
- その sample accuracy を sample 数で平均

です。

category 別も同様で、各 sample の category accuracy / F1 / BLEU / LLM を平均しています。

### 12.3 official との差分

official は per-question score を category ごとに足し込み、質問数で割る pooled average です。  
一方 MAGMA の複数 sample 集計は sample-level macro average です。

そのため、sample ごとの質問数や category 分布が違うと、official と MAGMA で aggregate 値は一致しません。

## 13. official との差分一覧

差分を一覧にすると次の通りです。

| 観点 | official | MAGMA |
| --- | --- | --- |
| 既定評価範囲 | 全 sample / 全 QA / category 1-5 | sample 0、先頭 50 問、category 1-4 |
| 主 judge | deterministic F1 | exact match, set-based F1, BLEU-1, LLM judge 併記 |
| `Accuracy` の意味 | official 文脈では per-question score の平均に近い | exact-match accuracy |
| LLM judge | 使わない | continuous 0-1 score を使う |
| best-of-N | なし | 既定で best-of-3 |
| gold の利用 | judge のみ | judge に加え best-of-N 選抜にも gold を使用 |
| category 1 | comma-split multi-answer F1 | 特別扱いなし。一般 metric を適用 |
| category 3 | `;` より前だけ採点 | 特別扱いなし |
| category 5 生成 | `(a)/(b)` 二択化 | free-form + entity mismatch 補正 |
| category 5 採点 | official 固有実装 | unanswerable なら 1、そうでなければ 0 |
| overall | category 1-5 を含む | default では 1-4 のみ。さらに `without_category5` も別計算 |
| 複数 sample 集計 | QA 単位の pooled average | sample 単位の単純平均 |
| retrieval 集計 | recall 系あり | official 風 recall 集計なし |

## 14. KIOKU 側で読むときの注意

MAGMA の LoCoMo 結果を引用するときは、少なくとも次を明記した方が安全です。

1. official 再現ではないこと
2. default では category 5 を除外していること
3. default では subset evaluation であること
4. `Accuracy` が exact match であること
5. best-of-3 + gold-aware reranking が入っていること

特に 4 と 5 は結果の解釈を大きく変えます。  
MAGMA の数値を official LoCoMo の F1 表とそのまま横並び比較するのは危険です。

## 15. KIOKU 実装への示唆

KIOKU 側で比較実験をするなら、MAGMA から学べる点と、そのまま真似しない方がよい点を分けるべきです。

学べる点:

- category ごとに answer prompt を変える
- category 5 を adversarial / entity-mismatch 問題として丁寧に扱う
- LLM judge を debug 用補助指標として持つ

そのまま採らない方がよい点:

- gold answer を best-of-N 選抜に使うこと
- official と異なる F1 を official 互換のように扱うこと
- subset evaluation を main result と混同すること

KIOKU で official 比較をするなら、

- official 互換 runner
- MAGMA 風 runner

を分けて実装するのが安全です。
