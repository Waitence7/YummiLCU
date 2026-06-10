namespace YummiLcu.App;

/// <summary>단일 인스턴스 Mutex + 기존 창 활성화 시그널.</summary>
public static class SingleInstanceHelper
{
    public const string MutexName = "YummiLcu.Agent.SingleInstance";
    public const string ActivateEventName = "YummiLcu.Agent.Activate";

    public static bool TryAcquireMutex(out Mutex? mutex)
    {
        mutex = new Mutex(true, MutexName, out var created);
        return created;
    }

    public static void SignalActivate()
    {
        try
        {
            using var ev = EventWaitHandle.OpenExisting(ActivateEventName);
            ev.Set();
        }
        catch (WaitHandleCannotBeOpenedException)
        {
            // 첫 인스턴스가 아직 리스너를 등록하지 않음
        }
    }

    public static void ListenForActivate(Action onActivate, CancellationToken ct)
    {
        using var ev = new EventWaitHandle(false, EventResetMode.AutoReset, ActivateEventName);
        while (!ct.IsCancellationRequested)
        {
            var signaled = ev.WaitOne(500);
            if (signaled && !ct.IsCancellationRequested)
                onActivate();
        }
    }
}
