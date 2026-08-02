# -*- coding: utf-8 -*-
"""重新打 graph.py 和 graphrag.py 的补丁（从文件读取源码，避免 heredoc 转义问题）"""
import io
import sys

PKG = r"C:/Users/pikachu/AppData/Roaming/Python/Python311/site-packages/codegraph_mcp"

TOKEN_RE = r"[\s,;:!?。，；：！？、（）()\[\]{}=|\\/<>]+"

# ============ graph.py ============
gp = PKG + "/core/graph.py"
src = io.open(gp, encoding="utf-8").read()

# --- patch 1: resolve_entity_id file::name 分支提前 ---
old1 = '''        # Try qualified_name suffix match
        cursor = await self._connection.execute(
            f"""
            SELECT id FROM entities
            WHERE qualified_name LIKE ?{type_filter}
            ORDER BY LENGTH(id) ASC
            LIMIT 10
            """,
            [f"%{entity_id}", *params],
        )
        rows = await cursor.fetchall()
        if len(rows) == 1:
            return rows[0][0]
        elif len(rows) > 1:
            # Multiple matches - ambiguous, return None
            return None

        # Try file::name pattern
        if "::" in entity_id:
            parts = entity_id.rsplit("::", 1)
            file_part, name_part = parts[0], parts[1]
            cursor = await self._connection.execute(
                f"""
                SELECT id FROM entities
                WHERE name = ? AND file_path LIKE ?{type_filter}
                ORDER BY LENGTH(id) ASC
                LIMIT 10
                """,
                [name_part, f"%{file_part}%", *params],
            )
            rows = await cursor.fetchall()
            if rows:
                return rows[0][0]

        return None'''

new1 = '''        # Try file::name pattern FIRST (fix: 旧实现先做 qualified_name 后缀
        # 匹配，同名实体多行时直接判定 ambiguous 返回 None，永远走不到
        # file::name 分支。现将 file::name 提前，并支持正反斜杠/大小写/
        # 相对路径，多行同名时按文件路径精确匹配或取首个结果)
        if "::" in entity_id:
            parts = entity_id.rsplit("::", 1)
            file_part, name_part = parts[0], parts[1]
            if name_part:
                norm_file = file_part.replace("\\\\", "/").lstrip("./")
                pats = ["%/" + norm_file, "%" + norm_file + "%"]
                for pat in pats:
                    cursor = await self._connection.execute(
                        f"""
                        SELECT id FROM entities
                        WHERE name = ? AND
                              LOWER(REPLACE(file_path, '\\\\', '/')) LIKE ?
                              {type_filter}
                        ORDER BY LENGTH(id) ASC
                        LIMIT 10
                        """,
                        [name_part, pat.lower(), *params],
                    )
                    rows = await cursor.fetchall()
                    if len(rows) == 1:
                        return rows[0][0]
                    elif len(rows) > 1:
                        exact = await self._connection.execute(
                            f"""
                            SELECT id FROM entities
                            WHERE name = ? AND
                                  LOWER(REPLACE(file_path, '\\\\', '/')) = ?
                                  {type_filter}
                            ORDER BY LENGTH(id) ASC
                            LIMIT 10
                            """,
                            [name_part, norm_file.lower(), *params],
                        )
                        exact_rows = await exact.fetchall()
                        if exact_rows:
                            return exact_rows[0][0]
                        return rows[0][0]

        # Try qualified_name suffix match
        cursor = await self._connection.execute(
            f"""
            SELECT id FROM entities
            WHERE qualified_name LIKE ?{type_filter}
            ORDER BY LENGTH(id) ASC
            LIMIT 10
            """,
            [f"%{entity_id}", *params],
        )
        rows = await cursor.fetchall()
        if len(rows) == 1:
            return rows[0][0]
        elif len(rows) > 1:
            # 多行同名（同一文件内多个定义）：返回首个而非放弃
            return rows[0][0]

        return None'''

assert old1 in src, "graph.py old1 not found"
src = src.replace(old1, new1)

# --- patch 2: query() 分词多字段匹配 ---
old2 = '''        # Text search with different matching strategies
        if query.query:
            base_sql += " AND (name LIKE ? OR qualified_name LIKE ?)"
            params.extend([f"%{query.query}%", f"%{query.query}%"])

        base_sql += f" LIMIT {query.max_results * 2}"  # Get more for scoring

        cursor = await self._connection.execute(base_sql, params)
        rows = await cursor.fetchall()

        seen_ids: set[str] = set()
        for row in rows:
            entity = self._row_to_entity(row)
            if entity.id not in seen_ids:
                seen_ids.add(entity.id)
                all_entities.append(entity)

                # Calculate relevance score
                score = self._calculate_relevance_score(
                    entity, search_term
                )
                scores[entity.id] = score

                # Track community
                if len(row) > 10 and row[10] is not None:
                    communities[entity.id] = row[10]'''

new2 = '''        # Text search: tokenize query and match across multiple fields
        # (fix: 旧实现只对 name/qualified_name 做整句 LIKE 匹配，中文自然
        # 语言查询必然落空。现按空白/标点分词，对 name / qualified_name /
        # signature / docstring / file_path 多字段做 OR 匹配，中文注释可命中)
        tokens = self._tokenize_query(query.query)
        if tokens:
            conditions = []
            for token in tokens:
                escaped = (
                    token.replace("\\\\", "\\\\\\\\")
                    .replace("%", "\\\\%")
                    .replace("_", "\\\\_")
                )
                pattern = "%" + escaped + "%"
                conditions.append(
                    "(LOWER(name) LIKE ? ESCAPE '\\\\' OR "
                    "LOWER(qualified_name) LIKE ? ESCAPE '\\\\' OR "
                    "LOWER(COALESCE(signature, '')) LIKE ? ESCAPE '\\\\' OR "
                    "LOWER(COALESCE(docstring, '')) LIKE ? ESCAPE '\\\\' OR "
                    "LOWER(file_path) LIKE ? ESCAPE '\\\\')"
                )
                params.extend([pattern.lower()] * 5)
            base_sql += " AND (" + " OR ".join(conditions) + ")"

        base_sql += f" LIMIT {query.max_results * 2}"  # Get more for scoring

        cursor = await self._connection.execute(base_sql, params)
        rows = await cursor.fetchall()

        seen_ids: set[str] = set()
        for row in rows:
            entity = self._row_to_entity(row)
            if entity.id not in seen_ids:
                seen_ids.add(entity.id)
                all_entities.append(entity)

                # Calculate relevance score
                score = self._calculate_relevance_score(
                    entity, search_term, tokens
                )
                scores[entity.id] = score

                # Track community
                if len(row) > 10 and row[10] is not None:
                    communities[entity.id] = row[10]'''

assert old2 in src, "graph.py old2 not found"
src = src.replace(old2, new2)

# --- patch 3: _tokenize_query + _calculate_relevance_score 分词评分 ---
old3 = '''    def _calculate_relevance_score(
        self, entity: Entity, search_term: str
    ) -> float:
        """Calculate relevance score for an entity."""
        score = 0.0
        name_lower = entity.name.lower()
        qualified_lower = entity.qualified_name.lower()

        # Exact name match: highest score
        if name_lower == search_term:
            score += 1.0
        # Name starts with search term
        elif name_lower.startswith(search_term):
            score += 0.8
        # Name contains search term
        elif search_term in name_lower:
            score += 0.6

        # Qualified name bonus
        if search_term in qualified_lower:
            score += 0.2

        # Entity type bonus (functions/classes are often more relevant)
        if entity.type in (EntityType.FUNCTION, EntityType.CLASS):
            score += 0.1

        return min(score, 1.0)'''

new3 = '''    @staticmethod
    def _tokenize_query(query: str) -> list[str]:
        """Split query into search tokens (supports Chinese + English)."""
        if not query:
            return []
        import re

        tokens = re.split(TOKEN_RE, query)
        return [t for t in tokens if t]

    def _calculate_relevance_score(
        self, entity: Entity, search_term: str, tokens: list[str] | None = None
    ) -> float:
        """Calculate relevance score for an entity."""
        score = 0.0
        name_lower = entity.name.lower()
        qualified_lower = entity.qualified_name.lower()
        doc_lower = (entity.docstring or "").lower()
        sig_lower = (entity.signature or "").lower()

        if tokens:
            # Token-based scoring (fix: 分词评分，中文查询可命中 docstring)
            for token in tokens:
                if token in name_lower:
                    score += 0.6
                elif token in qualified_lower:
                    score += 0.3
                if token in sig_lower:
                    score += 0.2
                if token in doc_lower:
                    score += 0.15
                if token in str(entity.file_path).lower():
                    score += 0.1
            return min(score, 1.0)

        # Exact name match: highest score
        if name_lower == search_term:
            score += 1.0
        # Name starts with search term
        elif name_lower.startswith(search_term):
            score += 0.8
        # Name contains search term
        elif search_term in name_lower:
            score += 0.6

        # Qualified name bonus
        if search_term in qualified_lower:
            score += 0.2

        # Entity type bonus (functions/classes are often more relevant)
        if entity.type in (EntityType.FUNCTION, EntityType.CLASS):
            score += 0.1

        return min(score, 1.0)'''

assert old3 in src, "graph.py old3 not found"
src = src.replace(old3, new3)

# TOKEN_RE 定义：插入到文件顶部 import 之后（class GraphQuery 之前）
anchor = "import networkx as nx\n"
assert anchor in src, "graph.py anchor not found"
src = src.replace(
    anchor,
    anchor + '\n\n# 查询分词正则（中英文标点/空白分隔；避免在字符类中使用引号字符）\nTOKEN_RE = ' + repr(TOKEN_RE) + "\n",
    1,
)

io.open(gp, "w", encoding="utf-8").write(src)
print("graph.py re-patched OK")

# ============ graphrag.py ============
grp = PKG + "/core/graphrag.py"
src2 = io.open(grp, encoding="utf-8").read()

old4 = '''    async def _find_relevant_communities(
        self,
        query: str,
        communities: list[dict[str, Any]],
    ) -> list[dict[str, Any]]:
        """Find communities relevant to the query."""
        query_lower = query.lower()
        query_words = set(query_lower.split())

        scored = []
        for comm in communities:
            score = 0.0

            # Match against name
            if comm.get("name"):
                name_lower = comm["name"].lower()
                if any(word in name_lower for word in query_words):
                    score += 0.5

            # Match against summary
            if comm.get("summary"):
                summary_lower = comm["summary"].lower()
                matching_words = sum(
                    1 for word in query_words if word in summary_lower
                )
                score += matching_words * 0.2

            if score > 0:
                comm["score"] = min(score, 1.0)
                scored.append(comm)

        # Sort by score
        scored.sort(key=lambda x: -x.get("score", 0))

        # If no matches, return top communities by size
        if not scored and communities:
            for comm in communities[: self.max_communities]:
                comm["score"] = 0.3
                scored.append(comm)

        return scored[: self.max_communities]'''

new4 = '''    def _tokenize_query(self, query: str) -> list[str]:
        """Split query into search tokens (Chinese + English)."""
        if not query:
            return []
        import re

        tokens = re.split(TOKEN_RE, query.lower())
        return [t for t in tokens if t]

    async def _find_relevant_communities(
        self,
        query: str,
        communities: list[dict[str, Any]],
    ) -> list[dict[str, Any]]:
        """Find communities relevant to the query.

        (fix: 旧实现按整句 whitespace 切词，中文查询整句作为一个词，
        对英文社区名/摘要无法命中。现按标点分词后逐词 substring 匹配，
        中英文关键词均可命中；'None'/NULL 摘要视为无摘要)
        """
        query_words = self._tokenize_query(query)

        scored = []
        for comm in communities:
            score = 0.0

            # Match against name
            name_raw = comm.get("name") or ""
            if name_raw and name_raw not in ("None", "null"):
                name_lower = name_raw.lower()
                for word in query_words:
                    if word and word in name_lower:
                        score += 0.5
                        break

            # Match against summary
            summary_raw = comm.get("summary") or ""
            if summary_raw and summary_raw not in ("None", "null"):
                summary_lower = summary_raw.lower()
                matching_words = sum(
                    1 for word in query_words if word and word in summary_lower
                )
                score += matching_words * 0.2

            if score > 0:
                comm["score"] = min(score, 1.0)
                scored.append(comm)

        # Sort by score
        scored.sort(key=lambda x: -x.get("score", 0))

        # If no matches, return top communities by size
        if not scored and communities:
            for comm in communities[: self.max_communities]:
                comm["score"] = 0.3
                scored.append(comm)

        return scored[: self.max_communities]'''

assert old4 in src2, "graphrag.py old4 not found"
src2 = src2.replace(old4, new4)

old5 = '''        return [
            {
                "id": row[0],
                "level": row[1],
                "name": row[2],
                "summary": row[3],
                "member_count": row[4],
            }
            for row in rows
        ]'''
new5 = '''        def _clean(value: Any) -> Any:
            # (fix: 旧库中 summary/name 可能被写成字符串 'None'，视为空)
            if value is None:
                return None
            if isinstance(value, str) and value.strip().lower() in ("none", "null", ""):
                return None
            return value

        return [
            {
                "id": row[0],
                "level": row[1],
                "name": _clean(row[2]),
                "summary": _clean(row[3]),
                "member_count": row[4],
            }
            for row in rows
        ]'''

assert old5 in src2, "graphrag.py old5 not found"
src2 = src2.replace(old5, new5)

anchor2 = "from codegraph_mcp.core.graph import GraphEngine\n"
if anchor2 not in src2:
    anchor2 = "from typing import TYPE_CHECKING, Any\n"
assert anchor2 in src2, "graphrag.py anchor not found"
src2 = src2.replace(
    anchor2,
    anchor2 + '\n\n# 查询分词正则（中英文标点/空白分隔；避免在字符类中使用引号字符）\nTOKEN_RE = ' + repr(TOKEN_RE) + "\n",
    1,
)

io.open(grp, "w", encoding="utf-8").write(src2)
print("graphrag.py re-patched OK")
