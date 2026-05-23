using System.Windows;

namespace YummiLcu.App.Infrastructure.Animation;

public interface IAnimationManager
{
    Task FadeSwapAsync(FrameworkElement host, Func<Task> swap);
}
