using System.Text.Json;

namespace YummiLcu.Agent;

internal sealed class AgentConfig
{
    private static readonly JsonSerializerOptions JsonOpts = new() { WriteIndented = true };

    public string RelayPublicBaseUrl { get; set; } = "http://127.0.0.1:8790";
    public int AuthPollIntervalMs { get; set; } = 1500;
    /// <summary>lockfile 전체 경로.</summary>
    public string? LockfilePath { get; set; }
    /// <summary>닷지 후 자동 매칭 재시작 방지 (큐 취소).</summary>
    public bool PreventQueueAfterDodge { get; set; } = true;
    /// <summary>연결 시 기본 상메(𝗬𝘂𝗺𝗺𝗶 𝗖𝗹𝗶𝗲𝗻𝘁) 적용.</summary>
    public bool ApplyDefaultStatusOnConnect { get; set; } = true;
    /// <summary>시작 시 업데이트 확인 (예: https://yummi.duckdns.org/agent/version.json).</summary>
    public string? UpdateManifestUrl { get; set; }
    public bool CheckUpdatesOnStartup { get; set; } = true;
    /// <summary>true면 새 버전 시 zip 받아 자동 교체·재시작.</summary>
    public bool AutoUpdateEnabled { get; set; } = true;

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
        if (!File.Exists(path))
            return new AgentConfig();
        try
        {
            var json = File.ReadAllText(path);
            return JsonSerializer.Deserialize<AgentConfig>(json) ?? new AgentConfig();
        }
        catch
        {
            return new AgentConfig();
        }
    }

    public void Save()
    {
        var json = JsonSerializer.Serialize(this, JsonOpts);
        File.WriteAllText(ConfigFilePath, json);
    }

    public string WsUrl(string sessionId)
    {
        var baseUrl = RelayPublicBaseUrl.TrimEnd('/');
        var wsBase = baseUrl.Replace("https://", "wss://", StringComparison.OrdinalIgnoreCase)
            .Replace("http://", "ws://", StringComparison.OrdinalIgnoreCase);
        return $"{wsBase}/ws/agent?session_id={Uri.EscapeDataString(sessionId)}";
    }

    public string LoginUrl(string sessionId) =>
        $"{RelayPublicBaseUrl.TrimEnd('/')}/login?session_id={Uri.EscapeDataString(sessionId)}";

    public string AuthStatusUrl(string sessionId) =>
        $"{RelayPublicBaseUrl.TrimEnd('/')}/auth/status?session_id={Uri.EscapeDataString(sessionId)}";
}
