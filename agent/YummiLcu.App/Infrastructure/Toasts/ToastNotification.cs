using CommunityToolkit.Mvvm.ComponentModel;

namespace YummiLcu.App.Infrastructure.Toasts;

public partial class ToastNotification : ObservableObject
{
    public ToastNotification(ToastType type, string title, string message)
    {
        Type = type;
        Title = title;
        Message = message;
    }

    public Guid Id { get; } = Guid.NewGuid();
    public ToastType Type { get; }
    public string Title { get; }
    public string Message { get; }
    public DateTimeOffset CreatedAt { get; } = DateTimeOffset.Now;
    [ObservableProperty] private bool _isClosing;
}
