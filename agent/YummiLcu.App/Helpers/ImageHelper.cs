using System.IO;
using System.Windows.Media.Imaging;

namespace YummiLcu.App.Helpers;

public static class ImageHelper
{
    public static BitmapImage? FromBytes(byte[]? data)
    {
        if (data is null or { Length: 0 }) return null;
        try
        {
            var img = new BitmapImage();
            using var ms = new MemoryStream(data);
            img.BeginInit();
            img.CacheOption = BitmapCacheOption.OnLoad;
            img.StreamSource = ms;
            img.EndInit();
            img.Freeze();
            return img;
        }
        catch
        {
            return null;
        }
    }
}
