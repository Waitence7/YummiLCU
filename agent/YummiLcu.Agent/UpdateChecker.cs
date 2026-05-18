using System.Reflection;
using System.Text.Json;

namespace YummiLcu.Agent;

internal static class UpdateChecker
{
    private static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(8) };

    public static string CurrentVersion =>
        Assembly.GetExecutingAssembly().GetName().Version?.ToString(3) ?? "0.0.0";

    public static async Task<UpdateInfo?> CheckAsync(string manifestUrl)
    {
        try
        {
            var json = await Http.GetStringAsync(manifestUrl);
            var info = JsonSerializer.Deserialize<UpdateInfo>(json, new JsonSerializerOptions
            {
                PropertyNameCaseInsensitive = true,
            });
            if (info is null || string.IsNullOrWhiteSpace(info.Version))
                return null;
            if (Version.Parse(info.Version) <= Version.Parse(CurrentVersion))
                return null;
            return info;
        }
        catch
        {
            return null;
        }
    }

    internal sealed class UpdateInfo
    {
        public string Version { get; set; } = "";
        public string? Url { get; set; }
        public string? Notes { get; set; }
    }
}
