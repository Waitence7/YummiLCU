namespace YummiLcu.App.Contracts.LCU;

public interface ILcuSystem
{
    bool IsConnected { get; }
    Task ConnectAsync(CancellationToken cancellationToken = default);
    void Disconnect();
}
