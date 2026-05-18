using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;

namespace YummiLcu.Agent;

/// <summary>로컬 LCU HTTPS (lockfile 기반).</summary>
internal sealed class LcuClient : IDisposable
{
    private readonly HttpClient _http;

    public LcuClient(int port, string password)
    {
        var handler = new HttpClientHandler
        {
            ServerCertificateCustomValidationCallback = (_, _, _, _) => true,
        };
        _http = new HttpClient(handler) { BaseAddress = new Uri($"https://127.0.0.1:{port}") };
        var token = Convert.ToBase64String(Encoding.UTF8.GetBytes($"riot:{password}"));
        _http.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Basic", token);
    }

    public static (LcuClient? Client, string? Error) TryFromLockfile(string lockfilePath)
    {
        if (!File.Exists(lockfilePath))
            return (null, "파일 없음");
        try
        {
            var raw = ReadLockfileText(lockfilePath);
            if (raw is null)
                return (null, "lockfile 읽기 재시도 실패 (클라이언트가 파일을 잠금)");
            if (string.IsNullOrEmpty(raw))
                return (null, "lockfile 비어 있음 (클라이언트 로딩 중일 수 있음)");
            var parts = raw.Split(':');
            if (parts.Length < 5)
                return (null, $"형식 오류 (필드 {parts.Length}개): {raw[..Math.Min(raw.Length, 80)]}");
            var port = int.Parse(parts[2]);
            var password = parts[3];
            return (new LcuClient(port, password), null);
        }
        catch (Exception ex)
        {
            return (null, ex.Message);
        }
    }

    /// <summary>롤 클라이언트가 lockfile을 잠근 상태에서도 읽기 (FileShare.ReadWrite).</summary>
    private static string? ReadLockfileText(string lockfilePath)
    {
        const int maxAttempts = 8;
        for (var attempt = 0; attempt < maxAttempts; attempt++)
        {
            try
            {
                using var fs = new FileStream(
                    lockfilePath,
                    FileMode.Open,
                    FileAccess.Read,
                    FileShare.ReadWrite | FileShare.Delete);
                using var reader = new StreamReader(fs, Encoding.UTF8);
                return reader.ReadToEnd().Trim();
            }
            catch (IOException) when (attempt < maxAttempts - 1)
            {
                Thread.Sleep(250);
            }
        }
        return null;
    }

    /// <summary>환경 변수 YUMMI_LCU_LOCKFILE 로 경로 지정 가능.</summary>
    public static string? FindLockfilePath()
    {
        var overridePath = Environment.GetEnvironmentVariable("YUMMI_LCU_LOCKFILE");
        if (!string.IsNullOrWhiteSpace(overridePath) && File.Exists(overridePath))
            return overridePath;

        var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        var candidates = new List<string>();

        // 설치 경로 (LCU API — 보통 1KB, ProgramData 쪽 0KB 파일과 다름)
        foreach (var drive in new[] { "C", "D", "E", "F" })
        {
            candidates.Add($@"{drive}:\Riot Games\League of Legends\lockfile");
            candidates.Add($@"{drive}:\Program Files\Riot Games\League of Legends\lockfile");
        }

        candidates.Add(Path.Combine(localAppData, "Riot Games", "Riot Client", "Config", "lockfile"));
        candidates.Add(Path.Combine(localAppData, "Riot Games", "Riot Client", "lockfile"));
        candidates.Add(Path.Combine(localAppData, "Riot Games", "League of Legends", "lockfile"));

        foreach (var path in candidates)
        {
            if (File.Exists(path))
                return path;
        }

        return null;
    }

    public static IReadOnlyList<string> DescribeSearchPaths()
    {
        var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        return new[]
        {
            @"C:\Riot Games\League of Legends\lockfile",
            @"D:\Riot Games\League of Legends\lockfile",
            Path.Combine(localAppData, "Riot Games", "Riot Client", "Config", "lockfile"),
        };
    }

    private static StringContent JsonBody(string json = "{}") =>
        new(json, Encoding.UTF8, "application/json");

    public async Task<bool> PostAsync(string path) =>
        (await _http.PostAsync(path, JsonBody())).IsSuccessStatusCode;

    public async Task<bool> DeleteAsync(string path) =>
        (await _http.DeleteAsync(path)).IsSuccessStatusCode;

    public async Task<bool> PutJsonAsync(string path, string json) =>
        (await _http.PutAsync(path, JsonBody(json))).IsSuccessStatusCode;

    public async Task<JsonDocument?> GetJsonAsync(string path)
    {
        try
        {
            var res = await _http.GetAsync(path);
            if (!res.IsSuccessStatusCode)
                return null;
            var text = await res.Content.ReadAsStringAsync();
            return JsonDocument.Parse(text);
        }
        catch
        {
            return null;
        }
    }

    public async Task<string?> GetGameflowPhaseAsync()
    {
        var doc = await GetJsonAsync("/lol-gameflow/v1/gameflow-phase");
        if (doc is null)
            return null;
        var phase = doc.RootElement.GetString();
        doc.Dispose();
        return phase;
    }

    public async Task<bool> SetStatusMessageAsync(string statusMessage)
    {
        var me = await GetJsonAsync("/lol-chat/v1/me");
        if (me is null)
            return false;
        try
        {
            using var stream = new MemoryStream();
            using (var writer = new Utf8JsonWriter(stream))
            {
                writer.WriteStartObject();
                foreach (var prop in me.RootElement.EnumerateObject())
                {
                    if (prop.NameEquals("statusMessage"))
                        continue;
                    prop.WriteTo(writer);
                }
                writer.WriteString("statusMessage", statusMessage);
                writer.WriteEndObject();
            }
            var json = Encoding.UTF8.GetString(stream.ToArray());
            return await PutJsonAsync("/lol-chat/v1/me", json);
        }
        finally
        {
            me.Dispose();
        }
    }

    public void Dispose() => _http.Dispose();
}
