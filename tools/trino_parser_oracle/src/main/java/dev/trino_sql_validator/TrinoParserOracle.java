package dev.trino_sql_validator;

import io.trino.sql.parser.ParsingException;
import io.trino.sql.parser.SqlParser;
import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.Base64;

public final class TrinoParserOracle {
    private TrinoParserOracle() {}

    public static void main(String[] args) throws Exception {
        SqlParser parser = new SqlParser();
        BufferedReader reader = new BufferedReader(
                new InputStreamReader(System.in, StandardCharsets.UTF_8));
        String line;
        while ((line = reader.readLine()) != null) {
            String[] fields = line.split("\\t", 3);
            if (fields.length != 3) {
                throw new IllegalArgumentException("expected id, entry point, and base64 SQL");
            }
            String id = fields[0];
            String entryPoint = fields[1];
            String sql = new String(Base64.getDecoder().decode(fields[2]), StandardCharsets.UTF_8);
            try {
                parse(parser, entryPoint, sql);
                System.out.println(id + "\tACCEPTED");
            }
            catch (ParsingException error) {
                String message = Base64.getEncoder().encodeToString(
                        error.getMessage().getBytes(StandardCharsets.UTF_8));
                System.out.println(id + "\tREJECTED\t" + error.getClass().getName()
                        + "\t" + error.getLineNumber() + "\t" + error.getColumnNumber()
                        + "\t" + message);
            }
            catch (Throwable error) {
                String message = Base64.getEncoder().encodeToString(
                        error.toString().getBytes(StandardCharsets.UTF_8));
                System.out.println(id + "\tINFRASTRUCTURE_ERROR\t"
                        + error.getClass().getName() + "\t" + message);
            }
        }
    }

    private static void parse(SqlParser parser, String entryPoint, String sql) {
        switch (entryPoint) {
            case "createStatement" -> parser.createStatement(sql);
            case "createExpression" -> parser.createExpression(sql);
            case "createType" -> parser.createType(sql);
            case "createFunctionSpecification" -> parser.createFunctionSpecification(sql);
            case "createRowPattern" -> parser.createRowPattern(sql);
            case "createPathSpecification" -> parser.createPathSpecification(sql);
            default -> throw new IllegalArgumentException("unknown entry point: " + entryPoint);
        }
    }
}
