using System;

namespace YummiGameBarWidget
{
    internal static class HudGeometry
    {
        // Game Bar bounds and XAML content can use different coordinate scales.
        // Derive the conversion from the same live view instead of assuming DIPs.
        internal static double TargetInHostUnits(double displayDips, double hostSize, double contentDips, double minimum, double maximum)
        {
            double hostUnitsPerDip = hostSize > 0 && contentDips > 0 ? hostSize / contentDips : 1.0;
            return Math.Max(minimum, Math.Min(maximum, displayDips * hostUnitsPerDip));
        }
    }
}
