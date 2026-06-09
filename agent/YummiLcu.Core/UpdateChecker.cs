using System.Reflection;
using System.Text.Json;

namespace YummiLcu.Core;

public static class UpdateChecker
{
    private static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(8) };

    public static string CurrentVersion =>
        Assembly.GetExecutingAssembly().GetName().Version?.ToString(3) ?? "0.0.0";

    public static async Task<UpdateInfo?> CheckAsync(string manifestUrl, CancellationToken ct = default)
    {
        if (!IsHttpsUrl(manifestUrl))
            return null;

        try
        {
            var json = await Http.GetStringAsync(manifestUrl.Trim(), ct);
            var info = JsonSerializer.Deserialize<UpdateInfo>(json, new JsonSerializerOptions
            {
                PropertyNameCaseInsensitive = true,
            });
            if (info is null || string.IsNullOrWhiteSpace(info.Version)) return null;
            if (Version.Parse(info.Version) <= Version.Parse(CurrentVersion)) return null;
            if (!string.IsNullOrWhiteSpace(info.Url) && !IsHttpsUrl(info.Url))
                return null;
            return info;
        }
        catch
        {
            return null;
        }
    }

    public static async Task<(bool Updating, string Message)> TryAutoUpdateAsync(
        AgentConfig config, CancellationToken ct = default)
    {
        if (!config.CheckUpdatesOnStartup || !config.AutoUpdateEnabled) return (false, "");
        if (string.IsNullOrWhiteSpace(config.UpdateManifestUrl)) return (false, "");

        var info = await CheckAsync(config.UpdateManifestUrl.Trim(), ct);
        if (info is null || (string.IsNullOrWhiteSpace(info.Url) && string.IsNullOrWhiteSpace(info.PatchUrl)))
            return (false, "");

        var (started, msg) = await AgentUpdater.DownloadAndApplyAsync(info, ct);
        if (!started) return (false, msg);

        await Task.Delay(500, ct);
        Environment.Exit(0);
        return (true, msg);
    }

    private static bool IsHttpsUrl(string url) =>
        Uri.TryCreate(url.Trim(), UriKind.Absolute, out var u) && u.Scheme == Uri.UriSchemeHttps;

    public sealed class UpdateInfo
    {
        public string Version { get; set; } = "";
        public string? Url { get; set; }
        public string? InstallerUrl { get; set; }
        public string? Notes { get; set; }
        /// <summary>zip SHA-256 hex (소문자). manifest 에 있으면 다운로드 후 검증.</summary>
        public string? Sha256 { get; set; }
        /// <summary>이전 버전→현재 버전 패치 zip (App.exe+Core.dll, ~2MB).</summary>
        public string? PatchUrl { get; set; }
        public string? PatchFrom { get; set; }
        public string? PatchSha256 { get; set; }

        public string? PreferredDownloadUrl =>
            !string.IsNullOrWhiteSpace(InstallerUrl) ? InstallerUrl : Url;
    }
}
