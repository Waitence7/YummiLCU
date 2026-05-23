namespace YummiLcu.App.Contracts.Discord;

public interface IDiscordRpcSystem
{
    bool IsRunning { get; }
    Task StartAsync(CancellationToken cancellationToken = default);
    Task StopAsync();
}
