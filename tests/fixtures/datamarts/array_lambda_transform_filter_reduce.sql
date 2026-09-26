SELECT
    order_id,
    raw_scores,
    -- Применение transform (умножаем каждый элемент на 1.1)
    transform(raw_scores, x -> x * 1.1) AS boosted_scores,
    -- Применение filter (оставляем значения > 50)
    filter(raw_scores, x -> x > 50) AS high_scores,
    -- Применение reduce (суммируем массив)
    reduce(raw_scores, 0, (s, x) -> s + x, s -> s) AS sum_scores,
    -- Cardinality и element_at
    cardinality(raw_scores) AS score_count,
    element_at(raw_scores, 1) AS first_score
FROM (
    SELECT
        order_id,
        ARRAY[10, 45, 80, 22, 99] AS raw_scores
    FROM raw_events.assessments
);