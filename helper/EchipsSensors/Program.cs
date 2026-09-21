// Вспомогательный процесс датчиков для Echips Hardware Check.
// Раз в интервал печатает в stdout одну JSON-строку со всеми показаниями
// LibreHardwareMonitor (температуры, обороты, напряжения, мощность, частоты).
// Завершается, когда закрывается stdin (родительское приложение вышло).
using System;
using System.Collections.Generic;
using System.Threading;
using System.Web.Script.Serialization;
using LibreHardwareMonitor.Hardware;

internal class UpdateVisitor : IVisitor
{
    public void VisitComputer(IComputer computer) { computer.Traverse(this); }
    public void VisitHardware(IHardware hardware)
    {
        hardware.Update();
        foreach (IHardware sub in hardware.SubHardware) sub.Accept(this);
    }
    public void VisitSensor(ISensor sensor) { }
    public void VisitParameter(IParameter parameter) { }
}

internal static class Program
{
    private static void Collect(IHardware hw, List<Dictionary<string, object>> list)
    {
        foreach (ISensor s in hw.Sensors)
        {
            if (!s.Value.HasValue) continue;
            var d = new Dictionary<string, object>();
            d["hw"] = hw.Name;
            d["hwType"] = hw.HardwareType.ToString();
            d["name"] = s.Name;
            d["type"] = s.SensorType.ToString();
            d["value"] = (double)s.Value.Value;
            if (s.Min.HasValue) d["min"] = (double)s.Min.Value;
            if (s.Max.HasValue) d["max"] = (double)s.Max.Value;
            list.Add(d);
        }
        foreach (IHardware sub in hw.SubHardware) Collect(sub, list);
    }

    private static void Emit(JavaScriptSerializer ser, Dictionary<string, object> msg)
    {
        Console.Out.WriteLine(ser.Serialize(msg));
        Console.Out.Flush();
    }

    private static int Main(string[] args)
    {
        int interval = 1000;
        if (args.Length > 0) int.TryParse(args[0], out interval);
        if (interval < 250) interval = 250;

        // родитель закрыл stdin (вышел или упал) — выходим и мы, чтобы не висеть с драйвером
        var watcher = new Thread(() =>
        {
            try { while (Console.In.Read() != -1) { } } catch { }
            Environment.Exit(0);
        });
        watcher.IsBackground = true;
        watcher.Start();

        var ser = new JavaScriptSerializer { MaxJsonLength = int.MaxValue };
        Computer computer = null;
        try
        {
            computer = new Computer
            {
                IsCpuEnabled = true,
                IsGpuEnabled = true,
                IsMotherboardEnabled = true,
                IsControllerEnabled = true,
                IsMemoryEnabled = true,
                IsBatteryEnabled = true,
                IsStorageEnabled = false,
                IsNetworkEnabled = false,
                IsPsuEnabled = false
            };
            computer.Open();
        }
        catch (Exception e)
        {
            var err = new Dictionary<string, object>();
            err["ok"] = false;
            err["error"] = "Не удалось инициализировать датчики: " + e.Message;
            Emit(ser, err);
            return 2;
        }

        var visitor = new UpdateVisitor();
        while (true)
        {
            var msg = new Dictionary<string, object>();
            try
            {
                computer.Accept(visitor);
                var list = new List<Dictionary<string, object>>();
                foreach (IHardware hw in computer.Hardware) Collect(hw, list);
                msg["ok"] = true;
                msg["sensors"] = list;
            }
            catch (Exception e)
            {
                msg["ok"] = false;
                msg["error"] = e.Message;
            }
            Emit(ser, msg);
            Thread.Sleep(interval);
        }
    }
}
