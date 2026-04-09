# LoCoMo の公式評価仕様メモ

## 1. 目的

この文書は、LoCoMo の**公開論文**と**公開実装**にもとづいて、現時点で確認できる「公式評価方法」を整理するためのメモです。  
ここでいう「公式」は、少なくとも次の公開物を指します。

- 論文: `Evaluating Very Long-Term Conversational Memory of LLM Agents`
- リポジトリ README
- `task_eval/evaluate_qa.py`
- `task_eval/evaluation.py`
- `task_eval/evaluation_stats.py`
- `task_eval/gpt_utils.py`

参照先:

- リポジトリ: <https://github.com/snap-research/locomo>
- README: <https://github.com/snap-research/locomo/blob/main/README.MD>
- 論文 PDF: <https://github.com/snap-research/locomo/blob/main/static/paper/locomo.pdf>
- 評価実装: <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation.py>
- 集計実装: <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation_stats.py>
- 実行エントリ: <https://github.com/snap-research/locomo/blob/main/task_eval/evaluate_qa.py>
- 回答生成実装: <https://github.com/snap-research/locomo/blob/main/task_eval/gpt_utils.py>

この文書では、次の 2 つを分けて整理します。

1. `judge`
   - 個々の質問に対して、予測回答をどう採点するか
2. `evaluation`
   - 個々の採点結果や retrieval 結果をどう集計するか

## 2. データセットと評価単位

LoCoMo の公開データは `data/locomo10.json` にあり、**1 sample = 1 本の長い会話**です。  
各 sample の `qa` 配列に複数の質問が含まれています。

各 QA サンプルは少なくとも次を持ちます。

- `question`
- `category`
- `evidence`
- `answer`

ただし `category = 5` については、後述の通り `answer` の有無が一貫していません。  
公開データの多くは `adversarial_answer` を持ちます。

データ:

- `locomo10.json`: <https://github.com/snap-research/locomo/blob/main/data/locomo10.json>

評価の基本単位は **QA 1 件ごとの採点** です。  
その後、category ごと、および overall に集計します。

## 3. category の意味

論文本文では、QA は次の 5 category に分かれると説明されています。

1. Single-hop
2. Multi-hop
3. Temporal reasoning
4. Open-domain knowledge / commonsense
5. Adversarial

論文の説明:

- Single-hop: 単一 session に基づく質問
- Multi-hop: 複数 session の情報統合が必要な質問
- Temporal: 時間情報や時系列推論が必要な質問
- Open-domain: 会話内容と外部知識や常識の統合が必要な質問
- Adversarial: 誤答を誘う unanswerable question

参照:

- 論文 PDF: <https://github.com/snap-research/locomo/blob/main/static/paper/locomo.pdf>

### 3.1 公開実装上の category ID 対応

論文本文は category 名を説明しますが、**数値 ID との対応表を明示していません**。  
公開実装の分岐から読むと、実質的には次の対応で使われています。

- `1`: Multi-hop
- `2`: Temporal
- `3`: Open-domain
- `4`: Single-hop
- `5`: Adversarial

根拠:

- `evaluation.py` の category 別採点分岐
- `gpt_utils.py` で `category == 2` の質問だけに date 用の追加指示を入れていること

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation.py>
- <https://github.com/snap-research/locomo/blob/main/task_eval/gpt_utils.py>

この対応は**論文本文から一意に読めるわけではなく、公開コードからの推定を含む**点に注意が必要です。

## 4. 回答生成時の前提

厳密には judge そのものではありませんが、公開評価コードは回答生成の前提を一部固定しています。  
judge の意味を解釈するために、ここも押さえておく必要があります。

### 4.1 category 1-4 の回答 prompt

公開コードでは、category 1-4 に対して次の短答 prompt を使います。

- 「上の context をもとに、短いフレーズで答える」
- 「可能なら context にある正確な語を使う」

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/gpt_utils.py>

### 4.2 temporal question の追加指示

`category == 2` の質問には、公開コード上で追加指示が付与されます。

- `Use DATE of CONVERSATION to answer with an approximate date.`

これは temporal question を回答させる際の official 実装上の条件です。

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/gpt_utils.py>

### 4.3 category 5 の回答形式

`category == 5` は adversarial question です。  
公開コードでは通常の短答ではなく、質問文を二択形式に変換してモデルへ渡します。

- `(a) Not mentioned in the conversation`
- `(b) 候補 answer`

この 2 つの順序はランダムです。  
モデルの出力が `(a)` / `(b)` 風なら、それを最終回答文字列へ戻してから採点します。

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/gpt_utils.py>

## 5. Judge 仕様

### 5.1 judge の性質

LoCoMo の公開 QA 評価は、**LLM judge ではありません**。  
公開実装は、正規化済み文字列に対する **token-level F1 ベースの決定的採点**です。

論文本文でも、長文回答の自動評価は難しいため、可能な限り会話中の語をそのまま答えるようにし、  
**F1 partial match metric** を使うと説明しています。

参照:

- 論文 PDF: <https://github.com/snap-research/locomo/blob/main/static/paper/locomo.pdf>
- `evaluation.py`: <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation.py>

### 5.2 正規化

公開実装 `normalize_answer()` では、少なくとも次の正規化を行います。

- 小文字化
- 句読点除去
- 冠詞 `a`, `an`, `the` の除去
- 接続詞 `and` の除去
- 連続空白の正規化

さらに F1 計算では token ごとに Porter stemming を適用します。

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation.py>

### 5.3 category ごとの採点

公開実装 `eval_question_answering()` は category ごとに採点方法を分けています。

### category 1: Multi-hop

Multi-hop は、prediction と gold をそれぞれカンマ区切りの複数 sub-answer に分割して扱います。

- prediction を `,` で split
- gold を `,` で split
- 各 gold 要素について、prediction 側の best-match F1 を取る
- それらの平均を最終スコアにする

つまり multi-hop は、**複数要素の部分一致を平均する特殊 F1** です。

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation.py>

### category 2, 3, 4: Temporal / Open-domain / Single-hop

これらは通常の token-level F1 です。

- prediction を normalize
- gold を normalize
- token overlap による precision / recall / F1 を計算

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation.py>

### category 3 の特殊ケース

`category == 3` では、gold answer に `;` が含まれる場合があります。  
公開実装はこの場合、**`;` より前だけ**を正解として採点します。

```python
if line['category'] == 3:
    answer = answer.split(';')[0].strip()
```

これは judge 実装上の明示的な special case です。

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation.py>

### category 5: Adversarial

公開実装では、category 5 は通常の F1 を使いません。  
prediction 文字列に次のどちらかが含まれていれば正解 1.0、そうでなければ 0.0 です。

- `no information available`
- `not mentioned`

つまり judge の本質は、**「unanswerable と正しく判断したか」** です。

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation.py>

### 5.4 question 単位の出力

公開 `evaluate_qa.py` は、各 QA ごとに最終スコアを JSON に書き戻します。  
保存されるキー名は model 名に依存しますが、中身は実質的に per-question F1 です。

例:

- `<model>_f1`
- `<model>_recall` ただし RAG のときだけ

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/evaluate_qa.py>

## 6. Evaluation 仕様

judge が返した per-question score を、公開実装では `evaluation_stats.py` で集計します。

### 6.1 answer 側の主指標

公開論文・README・集計コードから見る限り、LoCoMo QA の answer 側の主指標は次です。

- category 別平均 F1
- overall 平均 F1

論文の表でも、QA 結果は `F1-score for answer prediction` として提示されています。

参照:

- 論文 PDF: <https://github.com/snap-research/locomo/blob/main/static/paper/locomo.pdf>
- 集計実装: <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation_stats.py>

### 6.2 集計順序

公開集計コードでは、各 QA の score を category ごとに足し込み、  
最後にその category の質問数で割って平均を出します。

category の表示順は次です。

- `4`
- `1`
- `2`
- `3`
- `5`

これは実質的に次の順を意図していると読めます。

- Single-hop
- Multi-hop
- Temporal
- Open-domain
- Adversarial

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation_stats.py>

### 6.3 overall の定義

公開 `evaluation_stats.py` の overall は、**category 1-5 をすべて含めた単純平均**です。  
つまり adversarial も分母に入っています。

```python
total_v += acc_counts[k]
total_k += v
...
print("Overall accuracy: ", round(float(total_v)/total_k, 3))
```

ここで `keys = [4, 1, 2, 3, 5]` になっているため、overall は category 5 を除外していません。

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation_stats.py>

### 6.4 retrieval 評価

LoCoMo の各 QA には `evidence` が付きます。  
公開論文では、RAG 評価時に **answer F1** に加えて **recall@k** を報告するとされています。

ただし公開実装で実際に計算しているのは、一般的な IR の意味での `Recall@k` というより、  
**gold evidence coverage の平均**に近い指標です。

参照:

- 論文 PDF: <https://github.com/snap-research/locomo/blob/main/static/paper/locomo.pdf>
- `evaluation.py`: <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation.py>
- `evaluation_stats.py`: <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation_stats.py>

### 6.4.1 per-question retrieval score

公開実装では、RAG 実行時に `prediction_context` を保持し、  
それと gold `evidence` を比較して per-question の `recall_acc` を計算します。

基本の計算は次です。

- retrieved context IDs の集合を用意する
- `evidence` の各 ID が retrieved に含まれるかを確認する
- `一致した evidence 数 / 全 evidence 数` を per-question retrieval score にする

したがって 0/1 ではなく、**evidence の部分回収を許す平均 recall** です。

### 6.4.2 retrieval unit が session の場合

retrieved context ID が `S...` で始まる場合、公開実装は dialog-level ではなく  
session-level に丸めて `evidence` と比較します。

- retrieved: `S{session_id}`
- gold evidence: `D{session}:{turn}`

このときは gold evidence から session 番号だけを取り出して照合します。

### 6.4.3 evidence が空の場合

公開実装では、`evidence` が空、または context 情報がない場合、retrieval score を `1` として扱います。

```python
else:
    all_recall.append(1)
```

これは strict な retrieval benchmark というより、公開 script 上の便宜的な挙動です。

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation.py>

### 6.5 RAG の集計

RAG 実行時は、公開集計コードが次を出力します。

- category 別 answer F1
- overall answer F1
- category 別 recall
- overall recall

ここでいう recall は、前節の per-question retrieval score の平均です。

参照:

- <https://github.com/snap-research/locomo/blob/main/task_eval/evaluation_stats.py>

## 7. 公開物の不整合と注意点

LoCoMo の「公式仕様」を読むとき、次の不整合に注意が必要です。

### 7.1 category 5 の `answer` 不整合

公開 `gpt_utils.py` は category 5 の候補 answer を `qa['answer']` から取ろうとします。  
しかし公開 `locomo10.json` の category 5 の大半は `answer` を持たず、`adversarial_answer` しかありません。

つまり、**公開データと公開コードはそのままでは整合しません**。

確認できる状況は次です。

- category 5 の多く: `adversarial_answer` のみ
- 一部だけ: `answer` と `adversarial_answer` の両方を持つ

このため、category 5 の「完全な official 動作」は公開物だけではやや不明瞭です。

### 7.2 category 名と数値 ID の対応はコード依存

論文は reasoning type を自然言語で説明しますが、  
`1..5` との対応表は公開実装を見ないと確定しません。

したがって、LoCoMo の category 名をコードへ落とす際は、  
**論文の説明ではなく公開コードの分岐**を基準にした方が安全です。

### 7.3 retrieval 指標名と中身のズレ

論文や表では `Recall@k` と書かれていますが、公開コードで計算しているのは  
典型的な top-k recall というより **gold evidence coverage の平均**です。

そのため、LoCoMo の retrieval 指標を別実装で再現するときは、

- `hit@k`
- `MRR`
- `recall_all@k`

のような一般的 IR 指標と混同しないようにする必要があります。

## 8. KIOKU 実装への示唆

KIOKU 側で「LoCoMo 公式準拠」を目指すなら、最低限次を採るのが自然です。

1. Judge は `evaluation.py` のロジックを忠実に移植する
2. category 1/2/3/4/5 ごとの採点分岐を保持する
3. category 3 の `;` special case を保持する
4. category 5 は unanswerable 判定として扱う
5. 集計は category 別平均と overall 平均を出す
6. retrieval は公開コード準拠の evidence coverage 平均をまず実装する

一方で、category 5 のデータ不整合があるため、完全互換を名乗るには追加確認が必要です。  
後続研究の実装を調べると、この部分をどう解釈しているかが見えてくる可能性があります。
