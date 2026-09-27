WITH FUNCTION local_f(x BIGINT) RETURNS BIGINT RETURN x SELECT local_f(1);
SELECT local_f(2), missing_neighbor(3)
