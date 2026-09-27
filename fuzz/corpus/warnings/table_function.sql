SELECT * FROM TABLE(missing_ptf(d => DESCRIPTOR(x missing_type), v => missing_scalar(1)))
