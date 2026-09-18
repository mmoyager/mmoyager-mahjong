# 训练数据（不随仓库发布）

这个目录里的 `im-*.bin` / `sp-*.bin` 是自我博弈产生的经验文件，单文件 0.2~2.6 GB，
整个目录在开发机上约 18 GB，因此不进版本库。

它们**完全由本仓库的引擎生成**，不需要外部牌谱 —— 这也是本项目的规则：
不录入人类牌谱，人机从零自我博弈。要重建，最小的两条路径是：

```bash
# 1) 让规则老师打标签的模仿数据（当前最优权重的走棋状态 + 老师标签）
./target/release/mmj-selfplay generate --games 6000 --out data/selfplay/im-v15.bin \
  --checkpoint data/checkpoints/ck-ab.bin --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 6161 --dagger --teacher-v2 --label-kind imitation

# 2) 长期训练循环的首份 bootstrap 数据（用现成的 ck-ab 也可以直接用 1) 的输出）
.venv/bin/python python/trainer/loop.py --help | head -40
```

文件的二进制格式见 `rust/mmj-nn/src/data.rs`，Python 侧读取器在 `python/trainer/mmjdata.py`。
