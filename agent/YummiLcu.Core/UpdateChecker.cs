using System.Reflection;
using System.Text.Json;

namespace YummiLcu.Core;

public static class UpdateChecker
{
    private static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(8) };

    /// <summary>앱 exe 버전 (Core.dll 과 분리 — 자동 업데이트 루프 방지).</summary>
    public static string CurrentVersion =>
        (Assembly.GetEntryAssembly() ?? Assembly.GetExecutingAssembly())
        .GetName().Version?.ToString(3) ?? "0.0.0";

    public static async Task<UpdateInfo?> CheckAsync(string manifestUrl, CancellationToken ct = default)
    {
        if (!IsHttpsUrl(manifestUrl))
            return null;

        try
        {
            var json = await Http.GetStringAsync(manifestUrl.Trim(), ct);
            var manifest = JsonSerializer.Deserialize<ReleaseManifest>(json, new JsonSerializerOptions
            {
                PropertyNameCaseInsensitive = true,
            });
            var info = manifest?.SelectLegacy();
            if (info is null || string.IsNullOrWhiteSpace(info.Version)) return null;
            if (Version.Parse(info.Version) <= Version.Parse(CurrentVersion)) return null;
            if (!string.IsNullOrWhiteSpace(info.Url) && !IsHttpsUrl(info.Url))
                return null;
            if (!HasRequiredSha256(info))
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

    private static bool HasRequiredSha256(UpdateInfo info)
    {
        if (Version.TryParse(CurrentVersion, out var cur) &&
            Version.TryParse(info.PatchFrom ?? "", out var from) &&
            cur == from &&
            !string.IsNullOrWhiteSpace(info.PatchUrl))
            return !string.IsNullOrWhiteSpace(info.PatchSha256);
        return !string.IsNullOrWhiteSpace(info.Sha256);
    }

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

    /// <summary>
    /// v2 manifest keeps the C# agent under the legacy target and the Rust/Tauri
    /// agent under tauri.  Root fields are retained so old agents can still read
    /// a v2 manifest during a rolling deployment.
    /// </summary>
    public sealed class ReleaseManifest
    {
        public string? Version { get; set; }
        public string? Url { get; set; }
        public string? InstallerUrl { get; set; }
        public string? Notes { get; set; }
        public string? Sha256 { get; set; }
        public string? PatchUrl { get; set; }
        public string? PatchFrom { get; set; }
        public string? PatchSha256 { get; set; }
        public UpdateInfo? Legacy { get; set; }

        public UpdateInfo? SelectLegacy()
        {
            if (Legacy is { Version.Length: > 0 }) return Legacy;
            if (string.IsNullOrWhiteSpace(Version)) return null;
            return new UpdateInfo
            {
                Version = Version,
                Url = Url,
                InstallerUrl = InstallerUrl,
                Notes = Notes,
                Sha256 = Sha256,
                PatchUrl = PatchUrl,
                PatchFrom = PatchFrom,
                PatchSha256 = PatchSha256,
            };
        }
    }
}
