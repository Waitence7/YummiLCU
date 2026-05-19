using System.Windows;
using System.Windows.Media;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using YummiLcu.App.Helpers;
using YummiLcu.Core;
using YummiLcu.Core.Lcu;

namespace YummiLcu.App.ViewModels;

public partial class HomeViewModel : ObservableObject
{
    private readonly ILcuConnector _lcu;
    private readonly AgentConfig _config;

    [ObservableProperty] private string _summonerName = "—";
    [ObservableProperty] private string _summonerLevel = "";
    [ObservableProperty] private string _availability = "";
    [ObservableProperty] private string _statusMessage = StatusMessageHelper.DefaultYummiClient;
    [ObservableProperty] private ImageSource? _profileIcon;
    [ObservableProperty] private string _lockfilePath = "";
    [ObservableProperty] private bool _isLoading;
    [ObservableProperty] private bool _hasContent;
    [ObservableProperty] private double _contentOpacity = 1;
    [ObservableProperty] private bool _isTestMode;

    public HomeViewModel(ILcuConnector lcu, AgentConfig config)
    {
        _lcu = lcu;
        _config = config;
        _lockfilePath = config.LockfilePath ?? "";
        IsTestMode = lcu.IsTestMode;
        _lcu.TestModeChanged += enabled => IsTestMode = enabled;
        _lcu.ConnectionChanged += async connected =>
        {
            if (connected)
                await RunOnUiAsync(RefreshAsync);
        };
    }

    [RelayCommand]
    public async Task RefreshAsync()
    {
        IsLoading = true;
        HasContent = false;
        ContentOpacity = 0;

        try
        {
            await Task.Run(async () =>
            {
                var summoner = await _lcu.GetCurrentSummonerAsync().ConfigureAwait(false);
                var chat = await _lcu.GetChatMeAsync().ConfigureAwait(false);
                byte[]? iconBytes = null;
                if (summoner is not null)
                    iconBytes = await _lcu.GetProfileIconBytesAsync(summoner.ProfileIconId).ConfigureAwait(false);

                await RunOnUiAsync(() =>
                {
                    if (summoner is not null)
                    {
                        SummonerName = summoner.DisplayName;
                        SummonerLevel = $"레벨 {summoner.Level}";
                        ProfileIcon = ImageHelper.FromBytes(iconBytes);
                    }
                    if (chat is not null)
                    {
                        StatusMessage = chat.StatusMessage;
                        Availability = chat.Availability;
                    }
                    HasContent = summoner is not null || chat is not null;
                });
            }).ConfigureAwait(true);
        }
        finally
        {
            IsLoading = false;
            if (HasContent)
                ContentOpacity = 1;
        }
    }

    [RelayCommand]
    public async Task SaveStatusAsync()
    {
        var ok = await _lcu.SetStatusMessageAsync(StatusMessage).ConfigureAwait(true);
        if (ok)
            await Services.ModalOverlayService.ShowAlertAsync("상메", "상태 메시지가 저장되었습니다.").ConfigureAwait(true);
    }

    [RelayCommand]
    public async Task ConnectLcuAsync()
    {
        if (_lcu.IsTestMode)
        {
            await _lcu.ConnectAsync(_config).ConfigureAwait(true);
            await RefreshAsync().ConfigureAwait(true);
            return;
        }
        _config.LockfilePath = string.IsNullOrWhiteSpace(LockfilePath) ? null : LockfilePath.Trim();
        try { _config.Save(); } catch { /* ignore */ }
        await _lcu.ConnectAsync(_config).ConfigureAwait(true);
        await RefreshAsync().ConfigureAwait(true);
    }

    [RelayCommand]
    public void SaveLockfile()
    {
        _config.LockfilePath = string.IsNullOrWhiteSpace(LockfilePath) ? null : LockfilePath.Trim();
        try { _config.Save(); } catch { /* ignore */ }
    }

    private static Task RunOnUiAsync(Action action)
    {
        var d = Application.Current?.Dispatcher;
        if (d is null || d.CheckAccess())
        {
            action();
            return Task.CompletedTask;
        }
        return d.InvokeAsync(action).Task;
    }

    private static async Task RunOnUiAsync(Func<Task> action)
    {
        var d = Application.Current?.Dispatcher;
        if (d is null || d.CheckAccess())
            await action();
        else
            await d.InvokeAsync(action);
    }
}
