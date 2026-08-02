# -*- coding: utf-8 -*-
"""验证 codegraph 修复效果（直接调用修复后的包代码）"""
import asyncio
import sys
from pathlib import Path

sys.path.insert(0, r"C:/Users/pikachu/AppData/Roaming/Python/Python311/site-packages")

REPO = Path(r"C:/Users/pikachu/Desktop/mathset")


async def main():
    from codegraph_mcp.core.graph import GraphEngine, GraphQuery

    engine = GraphEngine(REPO)
    await engine.initialize()
    try:
        # ---- 测试 1: resolve_entity_id file::name ----
        eid = await engine.resolve_entity_id("questions.rs::create_question")
        print("[1] resolve_entity_id('questions.rs::create_question') ->", eid)

        # ---- 测试 2: query_codebase 中文查询 ----
        r1 = await engine.query(GraphQuery(query="AI 录入功能", max_results=10))
        print(f"[2] query('AI 录入功能') -> {len(r1.entities)} 个实体")
        for e in r1.entities[:5]:
            print(f"    - {e.type.value}: {e.qualified_name} (score={r1.scores.get(e.id):.2f})")

        # ---- 测试 3: query_codebase 英文查询（对照）----
        r2 = await engine.query(GraphQuery(query="login 认证 JWT", max_results=10))
        print(f"[3] query('login 认证 JWT') -> {len(r2.entities)} 个实体")
        for e in r2.entities[:5]:
            print(f"    - {e.type.value}: {e.qualified_name} (score={r2.scores.get(e.id):.2f})")

        # ---- 测试 4: 路径匹配（模拟 get_file_structure）----
        for p in ["src/ai/deepseek.rs", "src\\ai\\deepseek.rs",
                  "C:/Users/pikachu/Desktop/mathset/src/ai/deepseek.rs"]:
            norm = str(Path(p).resolve() if not p.startswith("src") else (REPO / p)).replace("\\", "/").lower()
            cur = await engine._connection.execute(
                "SELECT COUNT(*) FROM entities WHERE LOWER(REPLACE(file_path, '\\', '/')) = ?",
                (norm,),
            )
            cnt = (await cur.fetchone())[0]
            print(f"[4] file_path 规范化匹配 {p!r} -> {cnt} 个实体")
    finally:
        await engine.close()

    # ---- 测试 5: 社区检测 + name/summary 生成 ----
    from codegraph_mcp.core.community import CommunityDetector

    engine2 = GraphEngine(REPO)
    await engine2.initialize()
    try:
        detector = CommunityDetector()
        result = await detector.detect(engine2)
        print(f"[5] 社区检测 -> {len(result.communities)} 个社区, modularity={result.modularity:.3f}")
        from codegraph_mcp.core.graphrag import GraphRAGSearch
        search = GraphRAGSearch(engine2, use_llm=False)
        gres = await search.global_search("AI 录入 解析 题目")
        print(f"[6] global_search('AI 录入 解析 题目') -> searched={gres.communities_searched}, "
              f"confidence={gres.confidence:.2f}")
        for c in gres.relevant_communities[:3]:
            print(f"    community #{c['id']} name={c.get('name')!r} score={c.get('score')}")
        if gres.relevant_communities:
            c0 = gres.relevant_communities[0]
            print(f"    summary 前 100 字: {str(c0.get('summary'))[:100]!r}")
    finally:
        await engine2.close()


asyncio.run(main())
