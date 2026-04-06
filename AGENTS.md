# KIOKU

Agentic AI の記憶層の実装を行う。
複数 crate に分割し、以下のような構成とする。

- crates/core : 記憶層のドメイン層を実装するライブラリクレート
- crates/adapters/* : 記憶層の各種インターフェースの物理層を実装するライブラリクレート群
- crates/evaluate : 記憶層の評価プログラムを実装するクレート

.gitignore に追加してあり git 管理しないが、以下に参考にする論文の Python 実装を置く想定である。

- ./MAGMA : グラフ構造の記憶層の実装例
- ./EverMemOS : グラフ構造の記憶層の実装例
