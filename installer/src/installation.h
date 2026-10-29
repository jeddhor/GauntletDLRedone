// The installer's work, independent of its pages: finding the game disc,
// asking gdl_import whether a source is the supported disc, copying the
// payload (the game's programs and files) into the installation, running
// the import, and creating shortcuts. docs/decisions/0005-installer.md.
#pragma once

#include <QAtomicInt>
#include <QJsonObject>
#include <QList>
#include <QObject>
#include <QProcess>
#include <QString>

class QThread;

// A place the game disc may be: a mounted optical volume, an unmounted
// optical drive (Linux block device), an image file or a folder.
struct DiscCandidate {
    QString label;
    QString path;
};

QList<DiscCandidate> discCandidates();

// The payload directory beside the installer, its importer and its
// reference files.
QString payloadDir();
QString exeName(const QString &base);

// Disk space the installation needs: the payload plus the imported data.
qint64 requiredBytes(const QString &payload);

// gdl_import probe SOURCE: whether SOURCE is the supported disc.
class Probe : public QObject {
    Q_OBJECT
public:
    explicit Probe(QObject *parent = nullptr);
    void start(const QString &source);
    bool running() const;
signals:
    void finished(const QString &source, bool supported, const QString &message);
private:
    QProcess *m_proc = nullptr;
    QString m_source;
};

// Copy the payload into DEST, then gdl_import install SOURCE DEST.
class Installation : public QObject {
    Q_OBJECT
public:
    explicit Installation(QObject *parent = nullptr);
    ~Installation() override;
    void start(const QString &source, const QString &dest);
    void cancel();
    bool running() const { return m_running; }
signals:
    void step(const QString &title);
    void progress(double overall);
    void log(const QString &line);
    void finished(bool ok, const QString &message);
private:
    void runImporter();
    void handleLine(const QByteArray &line);
    void finish(bool ok, const QString &message);
    QString m_source, m_dest;
    QProcess *m_proc = nullptr;
    QThread *m_copier = nullptr;
    QAtomicInt m_cancel{0};
    bool m_running = false;
    bool m_done = false;
    // A first installation (no install.json before): what it copied is
    // removed again when it does not finish.
    bool m_fresh = false;
    QStringList m_copied;
    QString m_error;
    QByteArray m_buffer;
};

// The game's executable in an installation.
QString gameExecutable(const QString &dest);
// A menu entry and/or desktop shortcut for the installed game.
bool createShortcuts(const QString &dest, bool desktop, bool menu, QString *error);
// Remove leftovers of an interrupted import in DEST.
void cleanLeftovers(const QString &dest);
