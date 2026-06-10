using System.Drawing;
using System.Windows;
using System.Windows.Forms;
using YummiLcu.App.ViewModels;

namespace YummiLcu.App;

public partial class MainWindow : Window
{
    private NotifyIcon? _tray;
    private bool _reallyClose;

    public MainWindow()
    {
        InitializeComponent();
        Loaded += OnLoaded;
    }

    private void OnLoaded(object sender, RoutedEventArgs e)
    {
        _tray = new NotifyIcon
        {
            Text = "Yummi LCU Agent",
            Icon = SystemIcons.Application,
            Visible = true,
        };
        _tray.DoubleClick += (_, _) => RestoreFromTray();

        var menu = new ContextMenuStrip();
        menu.Items.Add("열기", null, (_, _) => RestoreFromTray());
        menu.Items.Add("종료", null, (_, _) =>
        {
            _reallyClose = true;
            Close();
        });
        _tray.ContextMenuStrip = menu;
    }

    public void RestoreFromTray()
    {
        Show();
        WindowState = WindowState.Normal;
        Activate();
        Topmost = true;
        Topmost = false;
        Focus();
    }

    public void HideToTray()
    {
        Hide();
        if (_tray is not null)
            _tray.Visible = true;
    }

    public void ShutdownApplication()
    {
        _reallyClose = true;
        System.Windows.Application.Current.Shutdown();
    }

    private void Window_StateChanged(object? sender, EventArgs e)
    {
        if (WindowState == WindowState.Minimized)
        {
            Hide();
            if (_tray is not null)
                _tray.Visible = true;
        }
    }

    private void Window_Closing(object? sender, System.ComponentModel.CancelEventArgs e)
    {
        if (!_reallyClose)
        {
            var connected = DataContext is AgentViewModel vm && vm.IsConnected;
            if (!connected)
            {
                _reallyClose = true;
            }
            else
            {
                e.Cancel = true;
                Hide();
                return;
            }
        }

        if (_tray is not null)
        {
            _tray.Visible = false;
            _tray.Dispose();
            _tray = null;
        }
    }
}
