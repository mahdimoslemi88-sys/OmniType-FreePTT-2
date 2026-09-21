# Rule 02: Documentation First Policy (قانون اولویت مستندسازی)
**Scope:** All Agents & Tasks  
**Authority:** Documentation Manager & Project Governance  

---

## 1. اصل اساسی (Fundamental Principle)
هیچ خط کدی نباید نوشته شود مگر آنکه مشخصات فنی، نیازمندی و معماری آن در مستندات پروژه ثبت و اعتبارسنجی شده باشد. کد همواره پیرو سند است، نه پیش‌درآمد آن.

## 2. چرخه حیات توسعه (Development Lifecycle)
1. **ثبت نیازمندی (Requirement):** توسط `business-analyst`
2. **تصمیم‌گیری معماری (ADR):** توسط `solution-architect`
3. **طراحی سناریوهای آزمون (Test Criteria):** توسط `qa-engineer`
4. **پیاده‌سازی کد (Implementation):** توسط متخصص مربوطه
5. **به‌روزرسانی هم‌گام اسناد:** شاخص کل `docs/INDEX.md` باید بلافاصله با هر سند جدید به‌روزرسانی شود.

## 3. هم‌گامی کد و سند (Doc-Code Synchronization)
هر زمان کدی تغییر کند، اسناد مرتبط با آن (API docs, diagrams, data dictionaries) باید در همان کامیت/تسک به‌روزرسانی شوند.
کد بدون سند، بدهی فنی حاد (Critical Debt) تلقی شده و رد می‌شود.
