// Gauntlet: Dark Legacy native port — installer.
//
//   GauntletDarkLegacy-Installer                 the wizard
//   GauntletDarkLegacy-Installer --batch --source DISC --dest FOLDER [--shortcuts]
//                                                the same work without pages
//                                                (automated checks); progress
//                                                on standard output
//
// The game's programs and reference files are in payload/ beside it.
#include "installation.h"
#include "wizard.h"

#include <QApplication>
#include <QCommandLineParser>
#include <QDir>
#include <QTextStream>

static int batch(const QString &source, const QString &dest, bool shortcuts)
{
    QTextStream out(stdout);
    Probe probe;
    bool supported = false;
    QString message;
    QEventLoop loop;
    QObject::connect(&probe, &Probe::finished, &loop, [&](const QString &, bool ok, const QString &m) {
        supported = ok;
        message = m;
        loop.quit();
    });
    probe.start(source);
    loop.exec();
    out << "probe: " << (supported ? "supported" : "not supported") << ": " << message << Qt::endl;
    if (!supported)
        return 2;
    Installation install;
    bool ok = false;
    QObject::connect(&install, &Installation::step, [&](const QString &s) { out << "step: " << s << Qt::endl; });
    int last = -1;
    QObject::connect(&install, &Installation::progress, [&](double f) {
        const int pct = int(f * 100);
        if (pct != last) {
            last = pct;
            out << "progress: " << pct << "%" << Qt::endl;
        }
    });
    QObject::connect(&install, &Installation::log, [&](const QString &s) { out << "log: " << s << Qt::endl; });
    QObject::connect(&install, &Installation::finished, &loop, [&](bool success, const QString &m) {
        ok = success;
        message = m;
        loop.quit();
    });
    install.start(source, dest);
    loop.exec();
    if (!ok) {
        out << "error: " << message << Qt::endl;
        return 1;
    }
    QString err;
    if (shortcuts && !createShortcuts(dest, true, true, &err)) {
        out << "error: " << err << Qt::endl;
        return 1;
    }
    out << "installed: " << QDir::toNativeSeparators(dest) << Qt::endl;
    return 0;
}

int main(int argc, char **argv)
{
    QApplication app(argc, argv);
    QApplication::setApplicationName(QStringLiteral("Gauntlet Dark Legacy Installer"));
    QApplication::setApplicationVersion(QStringLiteral(GDL_INSTALLER_VERSION));
    QCommandLineParser p;
    p.addHelpOption();
    p.addVersionOption();
    p.addOption({QStringLiteral("batch"), QStringLiteral("Install without pages.")});
    p.addOption({QStringLiteral("source"), QStringLiteral("The disc, drive or image."), QStringLiteral("path")});
    p.addOption({QStringLiteral("dest"), QStringLiteral("The install folder."), QStringLiteral("path")});
    p.addOption({QStringLiteral("shortcuts"), QStringLiteral("With --batch: create the menu and desktop shortcuts.")});
    p.process(app);
    if (p.isSet(QStringLiteral("batch"))) {
        if (!p.isSet(QStringLiteral("source")) || !p.isSet(QStringLiteral("dest"))) {
            QTextStream(stderr) << "--batch needs --source and --dest" << Qt::endl;
            return 2;
        }
        return batch(p.value(QStringLiteral("source")), QDir::cleanPath(QDir(p.value(QStringLiteral("dest"))).absolutePath()),
                     p.isSet(QStringLiteral("shortcuts")));
    }
    InstallerWizard w;
    w.show();
    return app.exec();
}
