using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Media.Effects;

namespace YummiLcu.App.Services;

/// <summary>메인 콘텐츠 Blur + 중앙 모달 오버레이.</summary>
public static class ModalOverlayService
{
    private static MainWindow? _window;
    private static BlurEffect? _blurEffect;
    private static int _openCount;

    public static void Initialize(MainWindow window)
    {
        _window = window;
        _blurEffect = new BlurEffect { Radius = 0 };
        window.MainContentRoot.Effect = _blurEffect;
    }

    public static async Task<bool> ShowAlertAsync(string title, string message, string confirmText = "확인")
    {
        if (_window is null) return false;

        var tcs = new TaskCompletionSource<bool>();
        await _window.Dispatcher.InvokeAsync(() =>
        {
            var panel = new StackPanel { MinWidth = 320 };
            panel.Children.Add(new TextBlock
            {
                Text = title,
                FontSize = 18,
                FontWeight = FontWeights.SemiBold,
                Foreground = (Brush)_window.FindResource("LcuAccent"),
                Margin = new Thickness(0, 0, 0, 8),
            });
            panel.Children.Add(new TextBlock
            {
                Text = message,
                TextWrapping = TextWrapping.Wrap,
                Foreground = (Brush)_window.FindResource("LcuText"),
                Margin = new Thickness(0, 0, 0, 16),
            });
            var btn = new Button
            {
                Content = confirmText,
                HorizontalAlignment = HorizontalAlignment.Right,
                MinWidth = 80,
                Padding = new Thickness(16, 8, 16, 8),
                Style = (Style)_window.FindResource("AccentButtonStyle"),
            };
            btn.Click += (_, _) =>
            {
                tcs.TrySetResult(true);
                CloseOverlay();
            };
            panel.Children.Add(btn);

            var card = new Border
            {
                Child = panel,
                Style = (Style)_window.FindResource("GamePanelBorder"),
                MaxWidth = 420,
            };

            OpenOverlay(card);
        });

        return await tcs.Task;
    }

    public static void OpenOverlay(UIElement content)
    {
        if (_window is null) return;

        _openCount++;
        if (_openCount == 1)
            AnimateBlur(to: 15, TimeSpan.FromSeconds(0.3));

        _window.ModalContentPresenter.Content = content;
        _window.ModalLayer.Visibility = Visibility.Visible;
    }

    public static void CloseOverlay()
    {
        if (_window is null) return;

        _openCount = Math.Max(0, _openCount - 1);
        _window.ModalLayer.Visibility = Visibility.Collapsed;
        _window.ModalContentPresenter.Content = null;

        if (_openCount == 0)
            AnimateBlur(to: 0, TimeSpan.FromSeconds(0.25));
    }

    private static void AnimateBlur(double to, TimeSpan duration)
    {
        if (_blurEffect is null) return;

        var anim = new DoubleAnimation
        {
            To = to,
            Duration = new Duration(duration),
            EasingFunction = new QuadraticEase { EasingMode = EasingMode.EaseOut },
        };
        _blurEffect.BeginAnimation(BlurEffect.RadiusProperty, anim, HandoffBehavior.SnapshotAndReplace);
    }
}
