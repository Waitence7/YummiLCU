using System.Diagnostics;

namespace YummiLcu.App.Services;

/// <summary>LeagueClientUx / LeagueClient 실행·종료 감지 (백그라운드 폴링).</summary>
public sealed class LeagueProcessMonitor : IDisposable
{
    private static readonly string[] ProcessNames =
    {
        "LeagueClientUx",
        "LeagueClient",
    };

    private CancellationTokenSource? _cts;
    private bool _wasRunning;

    public event Action? LeagueStarted;
    public event Action? LeagueExited;

    public bool IsLeagueRunning { get; private set; }

    public void Start(TimeSpan? interval = null)
    {
        Stop();
        _wasRunning = IsProcessRunning();
        IsLeagueRunning = _wasRunning;
        _cts = new CancellationTokenSource();
        var delay = interval ?? TimeSpan.FromSeconds(1.5);
        _ = Task.Run(() => WatchLoopAsync(_cts.Token, delay), _cts.Token);
    }

    public void Stop()
    {
        _cts?.Cancel();
        _cts?.Dispose();
        _cts = null;
    }

    private async Task WatchLoopAsync(CancellationToken ct, TimeSpan interval)
    {
        while (!ct.IsCancellationRequested)
        {
            try
            {
                var running = IsProcessRunning();
                if (running && !_wasRunning)
                {
                    _wasRunning = true;
                    IsLeagueRunning = true;
                    LeagueStarted?.Invoke();
                }
                else if (!running && _wasRunning)
                {
                    _wasRunning = false;
                    IsLeagueRunning = false;
                    LeagueExited?.Invoke();
                }
            }
            catch
            {
                // ignore transient process API errors
            }

            try
            {
                await Task.Delay(interval, ct);
            }
            catch (OperationCanceledException)
            {
                break;
            }
        }
    }

    private static bool IsProcessRunning()
    {
        foreach (var name in ProcessNames)
        {
            if (Process.GetProcessesByName(name).Length > 0)
                return true;
        }
        return false;
    }

    public void Dispose() => Stop();
}
