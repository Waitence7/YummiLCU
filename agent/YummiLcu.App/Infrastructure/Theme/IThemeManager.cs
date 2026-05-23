using YummiLcu.App.Services;

namespace YummiLcu.App.Infrastructure.Theme;

public interface IThemeManager
{
    AppTheme Current { get; }
    void Apply(AppTheme theme);
}
