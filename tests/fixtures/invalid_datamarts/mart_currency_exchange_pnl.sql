SELECT
    trader_id,
    currency_pair,
    SUM(CASE WHEN side = 'BUY' THEN base_amount ELSE -base_amount END) AS net_position,
    SUM(CASE WHEN side = 'BUY' THEN quote_amount ELSE -quote_amount END) AS net_cashflow,
    AVG_NULLS_LAST(rate) AS avg_execution_rate,
    WM_CONCAT(order_id) AS executed_orders_list
FROM lakehouse.trading.fx_executes
WHERE execute_time >= CURRENT_TIMESTAMP - INTERVAL '24' HOUR
GROUP BY trader_id, currency_pair;