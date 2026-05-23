using System.Windows;
using YummiLcu.App.Infrastructure.AppState;
using YummiLcu.App.Services;

namespace YummiLcu.App.Infrastructure.Animation;

public sealed class AnimationManager : IAnimationManager
{
    private readonly AppStateManager _state;

    public AnimationManager(AppStateManager state) => _state = state;

    public async Task FadeSwapAsync(FrameworkElement host, Func<Task> swap)
    {
        if (!_state.EnableUiAnimations)
        {
            await swap();
            host.Opacity = 1;
            return;
        }

        await NavigationService.FadeSwapAsync(host, swap);
    }
}
