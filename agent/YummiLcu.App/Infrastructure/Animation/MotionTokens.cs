using System.Windows;
using System.Windows.Media.Animation;

namespace YummiLcu.App.Infrastructure.Animation;

public static class MotionTokens
{
    public static readonly Duration Fast = new(TimeSpan.FromMilliseconds(120));
    public static readonly Duration Normal = new(TimeSpan.FromMilliseconds(220));
    public static readonly Duration Slow = new(TimeSpan.FromMilliseconds(360));

    public static IEasingFunction EaseOut { get; } =
        new QuadraticEase { EasingMode = EasingMode.EaseOut };
}
