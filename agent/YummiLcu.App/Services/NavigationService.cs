using System.Windows;
using System.Windows.Media.Animation;

namespace YummiLcu.App.Services;

public static class NavigationService
{
    public static async Task FadeSwapAsync(FrameworkElement host, Func<Task> swap)
    {
        var res = Application.Current.Resources;
        if (res["PageFadeOut"] is Storyboard fadeOut)
        {
            fadeOut = fadeOut.Clone();
            fadeOut.Begin(host);
            await Task.Delay(160);
        }
        else
        {
            host.Opacity = 0;
        }

        await swap();

        if (res["PageFadeIn"] is Storyboard fadeIn)
        {
            fadeIn = fadeIn.Clone();
            host.Opacity = 0;
            fadeIn.Begin(host);
        }
        else
        {
            host.Opacity = 1;
        }
    }
}
