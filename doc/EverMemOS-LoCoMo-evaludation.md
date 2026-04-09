# EverMemOS の LoCoMo 評価実装メモ

## 1. 目的

この文書は、`./EverOS/evaluation/cli.py` を起点に、EverMemOS が LoCoMo をどう評価しているかを整理するためのメモです。  
特に次の 2 点を分けて記述します。

1. EverMemOS が実際に何を実行しているか
2. `doc/LoCoMo-evaluation.md` に整理した LoCoMo 公式実装と何が違うか

ここでいう「EverMemOS の LoCoMo 評価」は、主に次のコードを指します。

- `EverOS/evaluation/cli.py`
- `EverOS/evaluation/config/datasets/locomo.yaml`
- `EverOS/evaluation/src/core/pipeline.py`
- `EverOS/evaluation/src/core/loaders.py`
- `EverOS/evaluation/src/core/stages/answer_stage.py`
- `EverOS/evaluation/src/evaluators/llm_judge.py`
- `EverOS/evaluation/config/prompts.yaml`
- `EverOS/evaluation/src/adapters/evermemos_adapter.py`
- `EverOS/evaluation/src/adapters/evermemos/config.py`
- `EverOS/evaluation/src/adapters/evermemos/stage3_memory_retrivel.py`
- `EverOS/evaluation/src/adapters/evermemos/stage4_response.py`
- `EverOS/evaluation/src/adapters/evermemos/prompts/answer_prompts.py`

## 2. 結論

先に要点だけ書くと、EverMemOS の LoCoMo 評価は official の忠実再現ではありません。

- 既定設定では `category 5` を**除外**しており、LoCoMo 全 1986 問ではなく `category 1-4` の 1540 問だけを評価する
- judge は official の deterministic token-level F1 ではなく、`gpt-4o-mini` による **LLM-as-a-Judge** である
- 最終 headline metric は F1 ではなく **accuracy** であり、しかも 3 回 judge した run accuracy の平均である
- 回答生成も official の短答 prompt ではなく、EverMemOS 独自の retrieval + CoT 型 prompt を使う
- official の category 別採点分岐
  - category 1 の comma-split multi-answer F1
  - category 3 の `;` 前半だけを使う special case
  - category 5 の `not mentioned` 判定
  を再現していない
- official の retrieval recall 集計も行っていない

したがって EverMemOS の LoCoMo 評価は、**LoCoMo 公式評価の再実装**というより、**LoCoMo 形式データを入力にした EverMemOS 独自の QA evaluation pipeline**として理解する方が正確です。

## 3. 実行入口と既定設定

実行入口は `EverOS/evaluation/cli.py` です。

```bash
uv run python -m evaluation.cli --dataset locomo --system evermemos
```

このとき `locomo` 用 dataset config と `evermemos` 用 system config が読み込まれます。

### 3.1 LoCoMo dataset config

`EverOS/evaluation/config/datasets/locomo.yaml` の既定値は次です。

- `evaluation.type: "llm_judge"`
- judge model: `gpt-4o-mini`
- `num_runs: 3`
- `filter_category: [5]`

`filter_category` は「含める category」ではなく、**除外する category** です。  
`Pipeline` 側の実装でも、指定 category を `dataset.qa_pairs` から除去しています。

したがって既定の LoCoMo 実行は、

1. LoCoMo 全 QA をロードする
2. その後で `category 5` を全部落とす
3. 残った `category 1-4` だけで search / answer / evaluate する

という挙動です。

### 3.2 実際の質問数

ローカルの `data/locomo10.json` を確認すると、QA 数は次でした。

- category 4: 841
- category 5: 446
- category 2: 321
- category 1: 282
- category 3: 96
- 合計: 1986

したがって `filter_category: [5]` の既定実行では、**1540 問**が評価対象です。

これは official の「category 1-5 を全部含めて category 別 / overall を出す」運用とは違います。

## 4. データ読み込みと前処理

### 4.1 LoCoMo format のまま読む

`load_dataset()` は `locomo` について conversion を挟まず、`data/locomo10.json` をそのまま `load_locomo_dataset()` へ渡します。  
1 conversation を 1 sample として読み、各 `qa` を独立した `QAPair` に展開します。

### 4.2 画像付き message の扱い

conversation 側は、`img_url` がある message に対して `blip_caption` を本文へ埋め込みます。

- 形式: `[{speaker} shared an image: {blip_caption}] {text}`

この点は official と完全一致とは限らず、EverMemOS 側の記憶構築対象は「素の LoCoMo text」ではなく、caption 合成後の content です。

### 4.3 QA の gold answer 読み込み

`_convert_locomo_qa_pair()` は gold answer を `qa_item.get("answer", "")` で読みます。  
つまり loader は **`answer` しか見ません**。

この点は category 5 で重要です。  
ローカル `data/locomo10.json` では category 5 の 446 問のうち 444 問で `answer` が空で、`adversarial_answer` だけがあります。

つまり、もし `filter_category: [5]` を外して category 5 を評価すると、EverMemOS 側では多くの adversarial question で

- official が参照する `adversarial_answer`
- official が行う `not mentioned` 判定

のどちらも再現されません。

既定設定では category 5 を除外しているため、この問題は表面化しません。  
しかし **category 5 を含めて official と比較しようとすると破綻する** ので、注意が必要です。

## 5. 評価フロー全体

`Pipeline` は次の 4 stage を順に実行します。

1. `add`
2. `search`
3. `answer`
4. `evaluate`

LoCoMo 既定実行では、概ね次の流れです。

1. LoCoMo conversation を読み込む
2. `filter_category: [5]` を適用して adversarial question を除外する
3. EverMemOS が会話から MemCell を抽出し index を構築する
4. 各 question に対して memory retrieval を行う
5. retrieval context から answer を生成する
6. 生成 answer を LLM judge で 3 回採点する
7. run ごとの accuracy を計算し、その平均と標準偏差を出す

official LoCoMo が answer 側 F1 と retrieval 側 recall を分けて集計するのに対し、EverMemOS の headline は **LLM judge accuracy** です。

## 6. retrieval の仕方

### 6.1 EverMemOS 固有の agentic retrieval

EverMemOS adapter は既定で `search.mode: "agentic"` を使います。  
`ExperimentConfig` の既定値でも `retrieval_mode: "agentic"` です。

`stage3_memory_retrivel.py` の `agentic_retrieval()` は次の multi-round process を取ります。

1. Round 1: hybrid retrieval で Top 20 を取得
2. Top 20 を rerank して Top 10 を作る
3. Top 10 に対して LLM で sufficiency check を行う
4. 十分ならその Top 10 を採用
5. 不十分なら refined query を生成して Round 2 retrieval を行う
6. 追加結果を merge / rerank して最終結果を返す

さらに `stage4_response.py` で answer を作るときは、最終 event IDs の **先頭 10 件**だけを context に使います。

### 6.2 official との違い

official LoCoMo の公開評価コードは、

- retrieval 成功度を `evidence` ベースで採点する
- answer judge 自体は token-level F1 を使う

という構造でした。

一方 EverMemOS は、

- retrieval を answer generation の前段処理として使う
- しかし official の `evidence` recall は計算しない
- retrieval 自体にも LLM sufficiency check と reranker を入れる

という構成です。

したがって retrieval 条件も official とは一致しません。

## 7. answer 生成の仕方

### 7.1 category 別 prompt ではない

official LoCoMo では category 1-4 に短答 prompt を使い、category 5 では二択 prompt に変換していました。  
しかし EverMemOS は category ごとに prompt を切り替えていません。

`stage4_response.py` は全 question に対して、`ANSWER_PROMPT` をそのまま使います。

### 7.2 CoT 型の長い answer prompt

`ANSWER_PROMPT` は次の性質を持ちます。

- step-by-step の reasoning を明示的に要求する
- cross-memory linking を要求する
- time reference calculation を要求する
- contradiction check を要求する
- `FINAL ANSWER:` を含む特定フォーマットを要求する

その後 `locomo_response()` は、生成文から `FINAL ANSWER:` より後ろだけを抜いて最終 answer とします。

つまり EverMemOS の LoCoMo answer generation は、

- official の「短いフレーズで答える」
- official の category 2 専用追加指示
- official の category 5 二択回答

とはかなり違います。

### 7.3 model も judge と answer で別

既定では

- answer generation: `openai/gpt-4.1-mini`
- judge: `gpt-4o-mini`

です。

この時点で、official の「同じ answer output を deterministic F1 で採点する」構図とは別系統です。

## 8. judge 実装

### 8.1 LLM-as-a-Judge

`evaluation.type` が `llm_judge` なので、LoCoMo の採点は `src/evaluators/llm_judge.py` が担当します。

judge prompt は `config/prompts.yaml` にあり、入力は次の 3 つです。

- question
- golden answer
- generated answer

judge は model に対して `CORRECT` / `WRONG` を JSON で返させ、`label == "CORRECT"` なら正解とみなします。

### 8.2 1 問 3 回判定

`num_runs: 3` なので、1 問ごとに judge を 3 回呼びます。  
保存形式は `judgment_1`, `judgment_2`, `judgment_3` です。

official のような deterministic judge ではないため、EverMemOS は **ばらつき込みの評価**になっています。

### 8.3 official の category 別 F1 分岐はない

EverMemOS judge は category ごとの特殊分岐を持ちません。  
すべての category を同じ `CORRECT / WRONG` binary judge にかけます。

そのため official にある次の処理は再現されません。

- category 1 の comma-split multi-answer F1
- category 2/3/4 の token-level F1
- category 3 で `;` より前だけを正解とする special case
- category 5 で `not mentioned` を検査する special case

特に official の主指標が F1 であるのに対し、EverMemOS の主指標は binary accuracy です。

## 9. 集計の仕方

### 9.1 overall metric

`LLMJudge.evaluate()` は、各 run について

1. 全 question の正誤を数える
2. `run_accuracy = correct_count / total_count` を計算する

という処理を行い、その後

- `mean_accuracy = mean(run_scores)`
- `std_accuracy = std(run_scores)`

を計算します。

つまり headline の `accuracy` は、**全 question を pooled した 1 回の採点結果**ではなく、  
**3 回の independent judge run の accuracy を平均した値**です。

### 9.2 category 別 metric

category 別も同様で、各 run の category accuracy を出してから、

- mean
- std
- individual_runs

を保存します。

official の category 別 score は per-question F1 を category ごとに足し上げる pooled average でした。  
EverMemOS は F1 でもなければ deterministic でもありません。

### 9.3 report に出る `correct`

`EvaluationResult.correct` には、厳密な run の integer correct count ではなく、

```python
int(mean_accuracy * len(answer_results))
```

が入ります。

したがって report 上の `Correct` は、実際のどれか 1 run の生カウントではなく、  
平均 accuracy を question 数に掛けて丸めた**派生値**です。

official の per-question score 合計とは意味が違います。

## 10. retrieval metric の欠如

official LoCoMo は answer 側だけでなく、`evidence` に基づく retrieval recall も持っていました。  
EverMemOS の evaluation framework では `EvaluationResult` に retrieval 指標がなく、`report.txt` に出るのも

- `Total Questions`
- `Correct`
- `Accuracy`

だけです。

つまり EverMemOS の LoCoMo 実装は、**official の retrieval evaluation を実装していません**。

## 11. official との主な差分

| 観点 | LoCoMo official | EverMemOS |
| --- | --- | --- |
| 評価対象 | category 1-5 全体 | 既定では category 5 を除外 |
| 既定質問数 | 全 QA | 1540 問 (`1986 - 446`) |
| answer prompt | 短答中心、category 2 追加指示、category 5 二択 | 全カテゴリ共通の CoT 型長文 prompt |
| judge | deterministic token-level F1 | LLM judge による CORRECT/WRONG |
| 主指標 | answer F1 | accuracy |
| category 1 | comma-split multi-answer F1 | 特別扱いなし |
| category 3 | `;` より前だけ採点 | 特別扱いなし |
| category 5 | `not mentioned` 判定 | 既定では除外。含めても official 再現ではない |
| retrieval metric | evidence recall を集計 | なし |
| 集計 | per-question score の pooled average | 3 run の accuracy 平均 |

## 12. category 5 についての補足

EverMemOS の既定設定は category 5 を除外しているため、表向きには大きな問題になりません。  
ただし「official と同条件にするため category 5 も入れたい」とすると、次の 3 つの問題が同時に発生します。

1. loader が `adversarial_answer` を読まず、`answer` しか使わない
2. judge が official の `not mentioned` special case を持たない
3. answer prompt も official の二択形式を使わない

したがって EverMemOS を少し設定変更しただけで official comparable な category 5 evaluation になるわけではありません。

## 13. まとめ

EverMemOS の LoCoMo 評価は、official の judge / 集計を再現していません。  
実態としては、

- LoCoMo 形式データを読む
- category 5 を除外する
- EverMemOS 独自 retrieval で context を作る
- 独自 CoT prompt で answer を生成する
- LLM judge を 3 回回して accuracy を平均する

という評価です。

そのため EverMemOS の数値を LoCoMo official の F1 表とそのまま横並びに比較するのは危険です。  
比較するなら少なくとも次を明記した方が安全です。

1. `category 5` を既定で除外していること
2. judge が LLM-as-a-Judge であり official F1 ではないこと
3. retrieval recall を評価していないこと
4. answer generation の prompt も official とは別物であること
