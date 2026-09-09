WITH raw_customer_activity AS (
    SELECT 
        c.customer_id,
        c.segment,
        a.event_time,
        a.event_type,
        -- Находим дату первой активности пользователя для когортного анализа
        MIN(CAST(a.event_time AS DATE)) OVER(PARTITION BY c.customer_id) as cohort_date,
        -- Считаем разницу в днях между текущим событием и предыдущим
        DATE_DIFF('day', 
            LAG(CAST(a.event_time AS DATE)) OVER(PARTITION BY c.customer_id ORDER BY a.event_time), 
            CAST(a.event_time AS DATE)
        ) as days_since_last_active
    FROM 
        postgres.crm.customers c  -- Таблица из реляционной БД
    JOIN 
        iceberg.events.page_views a -- Тяжелые логи из объектного хранилища (S3/MinIO)
        ON c.customer_id = a.user_id
    WHERE 
        a.event_time >= CURRENT_DATE - INTERVAL '90' DAY
),

aggregated_metrics AS (
    SELECT 
        customer_id,
        segment,
        cohort_date,
        COUNT(DISTINCT CAST(event_time AS DATE)) as total_active_days,
        -- Сложная агрегация: собираем распределение типов событий в JSON-подобный MAP
        histogram(event_type) as event_distribution,
        -- Фильтруем и собираем массив дней с аномально высокой задержкой ответа
        FILTER(ARRAY_AGG(days_since_last_active), x -> x IS NOT NULL AND x > 7) as gaps_over_week
    FROM 
        raw_customer_activity
    GROUP BY 
        customer_id, segment, cohort_date
)

-- Финальный слой витрины (Data Mart Level)
SELECT 
    customer_id,
    segment,
    cohort_date,
    total_active_days,
    -- Работа со структурой MAP: извлекаем конкретную метрику покупки «на лету»
    CARDINALITY(gaps_over_week) as churn_risk_signals,
    COALESCE(element_at(event_distribution, 'purchase'), 0) as purchase_count,
    COALESCE(element_at(event_distribution, 'add_to_cart'), 0) as cart_additions
FROM 
    aggregated_metrics
ORDER BY 
    purchase_count DESC;
