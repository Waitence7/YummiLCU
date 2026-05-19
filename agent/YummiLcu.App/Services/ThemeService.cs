using System.Windows;

namespace YummiLcu.App.Services;

public enum AppTheme { Cat, Cyber, Classic }

public static class ThemeService
{
    private static ResourceDictionary? _currentTheme;

    public static AppTheme Current { get; private set; } = AppTheme.Cat;

    public static void Apply(AppTheme theme)
    {
        var app = Application.Current;
        if (app is null) return;

        if (_currentTheme is not null)
            app.Resources.MergedDictionaries.Remove(_currentTheme);

        var uri = theme switch
        {
            AppTheme.Cyber => new Uri("Themes/CyberTheme.xaml", UriKind.Relative),
            AppTheme.Classic => new Uri("Themes/ClassicTheme.xaml", UriKind.Relative),
            _ => new Uri("Themes/CatTheme.xaml", UriKind.Relative),
        };

        _currentTheme = new ResourceDictionary { Source = uri };
        app.Resources.MergedDictionaries.Insert(0, _currentTheme);
        Current = theme;
    }
}
