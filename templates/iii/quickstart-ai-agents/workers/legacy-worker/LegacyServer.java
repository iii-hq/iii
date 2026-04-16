import com.sun.net.httpserver.HttpServer;
import com.sun.net.httpserver.HttpExchange;
import java.io.IOException;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.HashMap;
import java.util.Map;

/**
 * Legacy CRM System — a standalone Java HTTP server.
 *
 * This represents a system you CANNOT modify. It has no iii SDK.
 * Integration happens externally: a worker registers this server's
 * endpoints as iii functions, connecting it to the event bus
 * without touching this code.
 *
 * Endpoints:
 *   GET /api/status?company=<id>  — account status from legacy DB
 *   GET /api/health               — health check
 *
 * Build & run:
 *   javac LegacyServer.java
 *   java LegacyServer
 */
public class LegacyServer {

    // Simulated legacy database
    private static final Map<String, String> LEGACY_DB = new HashMap<>();

    static {
        LEGACY_DB.put("acme-corp", toJson("LEG-001", "active", "2019-03-15", "platinum"));
        LEGACY_DB.put("mega-inc", toJson("LEG-002", "active", "2020-07-01", "gold"));
        LEGACY_DB.put("small-biz", toJson("LEG-003", "trial", "2024-01-10", "basic"));
    }

    public static void main(String[] args) throws IOException {
        int port = Integer.parseInt(System.getenv().getOrDefault("PORT", "8080"));
        HttpServer server = HttpServer.create(new InetSocketAddress(port), 0);

        server.createContext("/api/status", exchange -> {
            if (!"GET".equalsIgnoreCase(exchange.getRequestMethod())) {
                sendResponse(exchange, 405, "{\"error\":\"method_not_allowed\"}");
                return;
            }

            String companyId = getQueryParam(exchange.getRequestURI().getQuery(), "company");
            String record = LEGACY_DB.get(companyId);

            if (record != null) {
                sendResponse(exchange, 200, record);
            } else {
                sendResponse(exchange, 404,
                    "{\"error\":\"not_found\",\"company\":\"" + escapeJson(companyId) + "\"}");
            }
        });

        server.createContext("/api/health", exchange -> {
            sendResponse(exchange, 200, "{\"status\":\"ok\",\"worker\":\"legacy-crm\"}");
        });

        server.setExecutor(null);
        server.start();
        System.out.println("Legacy CRM server running on port " + port);
    }

    private static String getQueryParam(String query, String key) {
        if (query == null) return "";
        for (String param : query.split("&")) {
            String[] pair = param.split("=", 2);
            if (pair.length == 2 && pair[0].equals(key)) {
                return pair[1];
            }
        }
        return "";
    }

    private static void sendResponse(HttpExchange exchange, int code, String body)
            throws IOException {
        byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().set("Content-Type", "application/json");
        exchange.sendResponseHeaders(code, bytes.length);
        try (OutputStream os = exchange.getResponseBody()) {
            os.write(bytes);
        }
    }

    private static String escapeJson(String value) {
        if (value == null) return "";
        return value.replace("\\", "\\\\").replace("\"", "\\\"");
    }

    private static String toJson(String legacyId, String status,
                                  String contractStart, String supportTier) {
        return "{" +
            "\"legacyId\":\"" + legacyId + "\"," +
            "\"status\":\"" + status + "\"," +
            "\"contractStart\":\"" + contractStart + "\"," +
            "\"supportTier\":\"" + supportTier + "\"," +
            "\"source\":\"legacy-crm\"" +
            "}";
    }
}
