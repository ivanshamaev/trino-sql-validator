# План разработки Jinja/dbt: анализ шаблонов без угадывания рендеринга

Дата: 2026-09-11.
База анализа: `v0.13.0`, commit `ad7124a17b22dc80ff2dbe319920024fac0b58f4`.
Статус: отдельный план; реализация Jinja/dbt ещё не начата.

Работа выделена из V014-02 [плана v0.14.0](v0.14.0_development_plan.md)
по решению пользователя. Она не входит в обязательный объём или release gates
v0.14.0; срок и версия выпуска определяются отдельно. При переносе изменены
только planning documents.

Цель: проверять доступные SQL-фрагменты в Jinja/dbt, обходить неизвестные
конструкции и явно сообщать о неполноте анализа. Библиотека не должна угадывать
результат macros, variables, adapter logic или преобразований dbt models.

Приоритет P0 внутри этого направления — устранение silent masking и явный
контракт неполноты; расширение контекстного анализа выполняется последующими
небольшими срезами. Файлы: Python API, Rust-native template analysis helpers,
тесты и документация. Runtime остаётся offline и не исполняет шаблоны.

## 1. Подтверждённые проблемы и уточнение анализа

Jinja обрабатывает исходный текст до SQL parser. SQL quotes, `--` и `/* ... */`
сами по себе не отключают Jinja. dbt прямо использует
`-- depends_on: {{ ref('upstream') }}` для обнаружения dependencies.
Поэтому первоначальное правило «не распознавать Jinja внутри SQL comments,
strings и dollar bodies» отзывается: оно не воспроизводит template language.
Источник: [dbt ref — forcing dependencies](https://docs.getdbt.com/reference/dbt-jinja-functions/ref#forcing-dependencies).

Для входа `-- {{ broken\nSELECT 1; -- }}\nSELECT 2` текущий count=1 подтверждён,
но требовать count=2 как результата реального rendering было неправильно:
Jinja region может пересекать SQL-comment newline. Здесь требуется diagnostic
о неподтверждённой/неподдержанной template structure и учёт всего пропущенного
span. Нельзя молча объявить полностью проверенным остаток файла.

Дополнительные probes текущего API:

| Исходник | Поведение 0.13.0 | Проблема |
| --- | --- | --- |
| `SELECT id FROM {{ anything() }}` | valid, opaque fragment заменён на `j` | успешная synthetic projection не подтверждает rendered SQL |
| `WHERE x {{ anything() }} 10` в SELECT | invalid | expression может выдавать оператор, это неизвестно |
| `{{ anything() }}` | invalid | весь SQL может генерироваться макросом |
| `{% if condition %} SELECT 1 {% else %} SELECT 2 {% endif %}` | invalid | две альтернативные ветки склеены в один SQL |
| `{{ configure_projection() }} SELECT 1` | valid | `startswith("config")` ошибочно удаляет произвольный macro call |
| `SEL{# comment #}ECT 1` | invalid | заполнение пробелами меняет token joining; Jinja comment удаляется |

## 2. Контракт: распознаём оболочку, не вычисляем содержимое

- Синтаксис delimiters и standard block boundaries задаётся явно выбранным
  template profile. Внутренние вызовы/выражения остаются непрозрачными;
  неизвестные filters, tests, names и extensions не исполняются.
- `ref`, `source`, `var`, `config`, пользовательские и adapter macros не получают
  специального результата по имени. Нельзя считать `config` пустой строкой,
  `ref` таблицей или loop ровно одной итерацией без внешнего явного контракта.
- Из SQL context можно определить ожидаемую роль фрагмента, например relation
  после FROM. Это требование SQL skeleton, а не утверждение, что macro вернёт
  именно relation. Такая проверка всегда маркируется как условная/частичная.
- Синтаксис Jinja `{# ... #}`, `raw` и whitespace controls относится к известной
  оболочке языка. Его обработка допустима по выбранному profile; неизвестные
  environment options/custom delimiters не восстанавливаются эвристически.
- SQL-комментарий и Jinja-комментарий различаются. В первом Jinja может
  выполняться; второй исключает заключённый в нём текст по grammar profile.
  Строковые SQL literals и language bodies также могут содержать template holes.

## 3. Рекомендуемая архитектура

1. **Template scanner.** Выделяет literal text, opaque expressions, control
   boundaries, Jinja comments и raw regions с исходными spans. Учитывает
   quoted delimiters/nested brackets внутри tags. SQL quote state не мешает
   обнаружению Jinja tags. Standard raw blocks отключают распознавание tags
   внутри себя; неизвестные block extensions дают отметку неопределённости.
2. **Структура шаблона.** Сохраняет if/elif/else alternatives и повторяемые
   regions без выбора ветки или числа итераций. Macro definitions и set/capture
   bodies не включаются автоматически в выходной SQL. Опциональная проверка
   их literal fragments не означает, что эти fragments будут выполнены.
3. **SQL skeleton / fragment checking.** Opaque nodes могут обозначать список,
   identifier fragment, operator, expression, clause или целый statement.
   Нейтральные parser placeholders допустимы только для проверки конкретной
   ожидаемой роли; они имеют synthetic provenance и не дают function/type
   warnings. Если роль неоднозначна, область расширяется до содержащего
   выражения/clause/statement и остаётся непроверенной. Перебор фиктивных
   значений до первого успешного parse не является доказательством valid.
4. **Отчёт о покрытии.** Возвращает checked/opaque/skipped spans, причины
   пропуска и диагностику с различием unconditional/conditional. Любое
   изменение неизвестным output кавычек, комментариев, delimiters или числа
   statements может расширить область неопределённости на остаток файла;
   один исходный `;` не гарантирует независимость следующего statement.

Для первой реализации допустимо пропустить весь template-dependent statement,
если корректная локализация меньшего фрагмента пока не поддерживается.
Ключевой критерий — отсутствие ложного SQL error от заглушки и отсутствие
ложного подтверждения полной проверки. Условные diagnostics известных SQL
fragments сохраняются и не теряются из-за такого пропуска.

## 4. Результат анализа и совместимость

Для честного ответа о шаблонах одного `valid: bool` недостаточно.
Предлагается отдельный `analyze_template()` → `TemplateAnalysis`, чтобы
сохранить существующие поля/tuple shape/сравнение `ValidationResult`:

```text
status: valid | invalid | indeterminate
coverage: full | partial | none
diagnostics: SQL/template diagnostics с исходными spans и условностью
checked_regions: fragments и основание их проверки
opaque_regions: regions и причины невозможности проверки
statement_count: integer | None
```

`valid` означает, что итоговый SQL определён без неизвестного rendering и
проверен целиком; `invalid` — доказанную ошибку, не вызванную synthetic
placeholder или неподдержанным template tag;
`indeterminate` — итог зависит от неизвестного rendering. Ошибка только в
потенциальной if-ветке остаётся условной диагностикой. Полностью динамическая
модель `{{ build_model() }}` получает `indeterminate`, coverage=none, без
выдуманного SQL error. Число statements не берётся из количества placeholders.

Текущие `validate(..., jinja="auto"/"mask")` сохраняют явно описанный legacy
best-effort contract: valid относится к проекции, не к compiled SQL. Они
используют исправленный общий scanner там, где это совместимо; изменения
ошибочного маскирования и невозможность выразить неопределённость одним bool
документируются. Новый API используется, когда нужна степень достоверности.
`jinja="reject"` сохраняет текущую передачу raw SQL parser без rendering.

Полная проверка конкретного rendered SQL доступна через обычный `validate()`
для текста, переданного вызывающей стороной. Библиотека сама не запускает
`dbt compile`: macros могут зависеть от warehouse introspection и выполнять
queries даже во время compilation.
Источник: [dbt compile](https://docs.getdbt.com/reference/commands/compile).

## 5. Пункты реализации и приёмка

- [ ] Убрать эвристики по именам macros, включая `startswith("config")`;
  зафиксировать разделение template syntax, opaque output и SQL skeleton.
- [ ] Реализовать template-first scanner с profile, spans, standard raw/comments,
  nested/quoted tag content и учётом unknown constructs; не исполнять шаблон.
- [ ] Сохранять structure control blocks без concatenation взаимоисключающих
  branches, имитации loop iterations или исполнения macro/capture bodies.
- [ ] Добавить conservative fragment analysis: typed holes только как ожидаемые
  SQL роли, synthetic provenance, explicit skipped regions и uncertainty bounds.
- [ ] Добавить отдельный `TemplateAnalysis` API с указанными статусами,
  Python types/native stubs/docs/tests, сохранив прежний ValidationResult.
- [ ] Использовать source map, а не только padding: Jinja comment removal и
  whitespace controls могут соединять SQL tokens; все diagnostics отображаются
  на исходный файл. При неизвестных whitespace options сохранять uncertainty.
- [ ] Проверить arbitrary macro names, operators/lists/clauses/full models,
  embedded identifiers, dbt fixture, conditional/loop/capture/raw/extension
  blocks, SQL comments/strings/dollar bodies, CRLF/Unicode и quoted delimiters.
- [ ] Проверить метаморфный контракт: замена имени/тела opaque expression при
  тех же границах и SQL context не меняет предположения о его результате;
  наличие parser placeholder не создаёт ошибку или warning исходного SQL.
- [ ] Обновить README о partial analysis, legacy masking и compiled input;
  никакая template-dependent область не исчезает из structured report молча.

Критерий: полностью динамические/неподдержанные модели признаются
неопределёнными, доступный SQL проверяется с объяснимыми границами, macro
semantics не угадываются, source positions сохранены. Положительный dbt fixture
сохраняет legacy поведение, а новый API явно показывает динамические regions.
Известный template syntax описан в
[Jinja Template Designer Documentation](https://jinja.palletsprojects.com/en/stable/templates/).

## 6. Порядок реализации и проверка совместимости

1. Зафиксировать контракт `TemplateAnalysis` и исходные reproduction cases;
   разделить подтверждённые ошибки, условные diagnostics и непроверенные spans.
2. Реализовать scanner и source map, затем сохранение структуры blocks.
3. Добавить conservative fragment checking и structured report; непрозрачные
   constructs не должны превращаться в выдуманные SQL errors.
4. Подключить отдельный Python API, stubs и документацию; проверить совместимость
   существующих `validate()` / `validate_file()` и Jinja modes.
5. Расширять покрытие SQL roles и сочетаний только с положительными,
   отрицательными и source-position regressions; выполнить Rust/Python CI gates.

Этот план не требует реализации `analyze_statements()` из V014-11. При наличии
общей statement metadata её можно переиспользовать, но порядок выпуска этих
двух возможностей независим. Тесты уже поддержанного dbt fixture остаются
регрессионным контрактом и в основной разработке библиотеки.
