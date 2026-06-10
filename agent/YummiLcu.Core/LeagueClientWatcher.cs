using YummiLcu.Core.Lcu;

namespace YummiLcu.Core;

/// <summary>롤 클라이언트 lockfile 등장/소멸 감시.</summary>
public sealed class LeagueClientWatcher
{
    private static readonly TimeSpan PollInterval = TimeSpan.FromMilliseconds(1500);

    private bool _present;
    private bool _suppressStartUntilAbsent;

    public event Action? LeagueClientStarted;
    public event Action? LeagueClientStopped;

    public void NotifyManualDisconnectWhileClientRunning() => _suppressStartUntilAbsent = true;

    public static string? ResolveLockfilePath(AgentConfig config)
    {
        var configured = config.ResolveLockfilePath();
        if (!string.IsNullOrWhiteSpace(configured) && File.Exists(configured))
            return configured;
        return LcuClient.FindLockfilePath();
    }

    public static bool IsClientPresent(AgentConfig config) =>
        IsClientPresent(() => ResolveLockfilePath(config));

    public static bool IsClientPresent(Func<string?> resolvePath)
    {
        var path = resolvePath();
        if (string.IsNullOrWhiteSpace(path) || !File.Exists(path))
            return false;
        return LcuClient.ReadLockfileSignature(path) is not null;
    }

    public async Task RunAsync(AgentConfig config, CancellationToken ct) =>
        await RunAsync(() => ResolveLockfilePath(config), ct);

    public async Task RunAsync(Func<string?> resolvePath, CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            var present = IsClientPresent(resolvePath);

            if (present && !_present)
            {
                _present = true;
                if (!_suppressStartUntilAbsent)
                    LeagueClientStarted?.Invoke();
            }
            else if (!present && _present)
            {
                _present = false;
                _suppressStartUntilAbsent = false;
                LeagueClientStopped?.Invoke();
            }

            try
            {
                await Task.Delay(PollInterval, ct);
            }
            catch (OperationCanceledException)
            {
                break;
            }
        }
    }
}
