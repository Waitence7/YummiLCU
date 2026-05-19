using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using YummiLcu.Core.Lcu;
using YummiLcu.Core.Lcu.Models;

namespace YummiLcu.App.ViewModels;

public partial class ChampSelectViewModel : ObservableObject
{
    private readonly ILcuConnector _lcu;
    private CancellationTokenSource? _pollCts;

    [ObservableProperty] private bool _sessionActive;
    [ObservableProperty] private string _phase = "";
    [ObservableProperty] private string _pickChampionIdText = "157";
    [ObservableProperty] private int _selectedActionId;

    public ObservableCollection<ChampSelectAction> Actions { get; } = new();
    public ObservableCollection<PerkPageInfo> PerkPages { get; } = new();

    public ChampSelectViewModel(ILcuConnector lcu) => _lcu = lcu;

    [RelayCommand]
    public async Task RefreshAsync()
    {
        var session = await _lcu.GetChampSelectSessionAsync();
        Actions.Clear();
        if (session is null || !session.IsActive)
        {
            SessionActive = false;
            Phase = "챔프 선택 중이 아님";
            return;
        }

        SessionActive = true;
        Phase = session.Phase;
        foreach (var a in session.Actions.Where(x => !x.Completed && x.IsInProgress))
            Actions.Add(a);
        if (Actions.Count > 0)
            SelectedActionId = Actions[0].Id;

        PerkPages.Clear();
        foreach (var p in await _lcu.GetPerkPagesAsync())
            PerkPages.Add(p);
    }

    [RelayCommand]
    public async Task PickAsync()
    {
        if (!int.TryParse(PickChampionIdText, out var champId) || SelectedActionId <= 0)
            return;
        await _lcu.PickChampionAsync(SelectedActionId, champId);
        await RefreshAsync();
    }

    [RelayCommand]
    public async Task ApplyPerkAsync(PerkPageInfo? page)
    {
        if (page is null) return;
        var ok = await _lcu.ApplyPerkPageAsync(page.Id).ConfigureAwait(true);
        await Services.ModalOverlayService.ShowAlertAsync(
            "룬 페이지",
            ok ? $"「{page.Name}」 적용을 요청했습니다." : "룬 페이지 적용에 실패했습니다.").ConfigureAwait(true);
    }

    [RelayCommand]
    public void StartPolling()
    {
        StopPolling();
        _pollCts = new CancellationTokenSource();
        var ct = _pollCts.Token;
        _ = Task.Run(async () =>
        {
            while (!ct.IsCancellationRequested)
            {
                try
                {
                    await System.Windows.Application.Current.Dispatcher.InvokeAsync(RefreshAsync);
                }
                catch { /* page closed */ }
                await Task.Delay(1500, ct);
            }
        }, ct);
    }

    [RelayCommand]
    public void StopPolling()
    {
        _pollCts?.Cancel();
        _pollCts?.Dispose();
        _pollCts = null;
    }
}
