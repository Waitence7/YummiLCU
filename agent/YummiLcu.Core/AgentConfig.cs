using System.Text.Json;

namespace YummiLcu.Core;

public sealed class AgentConfig
{
    private static readonly JsonSerializerOptions JsonOpts = new() { WriteIndented = true };

    public string RelayPublicBaseUrl { get; set; } = "https://yummi.duckdns.org";
    public int AuthPollIntervalMs { get; set; } = 1500;
    public string? LockfilePath { get; set; }
    public bool PreventQueueAfterDodge { get; set; } = true;
    public bool ApplyDefaultStatusOnConnect { get; set; } = true;
    public bool AutoAcceptMatch { get; set; }
    /// <summary>롤 클라이언트 실행 시 Relay 자동 연결, 종료 시 연결·앱 종료.</summary>
    public bool FollowLeagueClient { get; set; } = true;
    public string? UpdateManifestUrl { get; set; } = "https://yummi.duckdns.org/agent/version.json";
    public bool CheckUpdatesOnStartup { get; set; } = true;
    public bool AutoUpdateEnabled { get; set; }
    public int SavedSessionMaxAgeDays { get; set; } = 14;
    public bool RunAtWindowsStartup { get; set; }
    public bool UiTestMode { get; set; }

    public string ConfigFilePath => Path.Combine(AppContext.BaseDirectory, "agent.json");

    public string? ResolveLockfilePath()
    {
        if (string.IsNullOrWhiteSpace(LockfilePath))
            return null;
        return Environment.ExpandEnvironmentVariables(LockfilePath.Trim());
    }

    public static AgentConfig Load()
    {
        var path = Path.Combine(AppContext.BaseDirectory, "agent.json");
        AgentConfig cfg;
        if (!File.Exists(path))
            cfg = new AgentConfig();
        else
        {
            try
            {
                var json = File.ReadAllText(path);
                using var doc = JsonDocument.Parse(json);
                var hasFollowLeague = doc.RootElement.TryGetProperty("FollowLeagueClient", out _);
                cfg = JsonSerializer.Deserialize<AgentConfig>(json) ?? new AgentConfig();
                if (!hasFollowLeague)
                    cfg.FollowLeagueClient = true;
            }
            catch
            {
                cfg = new AgentConfig();
            }
        }
        cfg.EnsureSecureCommunication();
        return cfg;
    }

    /// <summary>공개 Relay·업데이트 URL 은 HTTPS(로컬 제외).</summary>
    public void EnsureSecureCommunication()
    {
        RelayPublicBaseUrl = EnforceHttpsIfPublic(RelayPublicBaseUrl);
        if (!string.IsNullOrWhiteSpace(UpdateManifestUrl))
            UpdateManifestUrl = EnforceHttpsIfPublic(UpdateManifestUrl.Trim());
    }

    private static string EnforceHttpsIfPublic(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri))
            return url;
        if (uri.Host is "localhost" or "127.0.0.1" or "::1")
            return url.TrimEnd('/');
        if (uri.Scheme == Uri.UriSchemeHttps)
            return url.TrimEnd('/');
        if (uri.Scheme == Uri.UriSchemeHttp)
        {
            var builder = new UriBuilder(uri)
            {
                Scheme = Uri.UriSchemeHttps,
                Port = uri.IsDefaultPort ? -1 : uri.Port,
            };
            return builder.Uri.ToString().TrimEnd('/');
        }
        return url;
    }

    public void Save()
    {
        var json = JsonSerializer.Serialize(this, JsonOpts);
        File.WriteAllText(ConfigFilePath, json);
    }

    public string WsUrl(string sessionId)
    {
        var baseUrl = EnforceHttpsIfPublic(RelayPublicBaseUrl.TrimEnd('/'));
        if (!Uri.TryCreate(baseUrl, UriKind.Absolute, out var uri))
            throw new InvalidOperationException("RelayPublicBaseUrl invalid");
        var wsScheme = uri.Scheme == Uri.UriSchemeHttps ? "wss" : "ws";
        var wsBase = $"{wsScheme}://{uri.Host}{(uri.IsDefaultPort ? "" : $":{uri.Port}")}";
        return $"{wsBase}/ws/agent?session_id={Uri.EscapeDataString(sessionId)}";
    }

    public string LoginUrl(string sessionId) =>
        $"{RelayPublicBaseUrl.TrimEnd('/')}/login?session_id={Uri.EscapeDataString(sessionId)}";

    public string AuthStatusUrl(string sessionId) =>
        $"{RelayPublicBaseUrl.TrimEnd('/')}/auth/status?session_id={Uri.EscapeDataString(sessionId)}";
}
