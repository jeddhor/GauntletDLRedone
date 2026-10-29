// The installer's pages: welcome, the game disc, the destination, the
// installation, the end.
#pragma once

#include <QWizard>
#include <QWizardPage>

class QCheckBox;
class QLabel;
class QLineEdit;
class QListWidget;
class QPlainTextEdit;
class QProgressBar;
class Installation;
class Probe;

enum PageId { WelcomePage, DiscPage, DestinationPage, InstallPage, DonePage };

class InstallerWizard : public QWizard {
    Q_OBJECT
public:
    explicit InstallerWizard(QWidget *parent = nullptr);
    QString source;      // the chosen disc (probed supported)
    QString destination; // the installation folder
    bool desktopShortcut = true;
    bool menuShortcut = true;
    void reject() override;
};

class DiscSelectPage : public QWizardPage {
    Q_OBJECT
public:
    explicit DiscSelectPage(InstallerWizard *w);
    void initializePage() override;
    bool isComplete() const override;
private:
    void rescan();
    void choose(const QString &path, const QString &label);
    InstallerWizard *m_wizard;
    QListWidget *m_list;
    QLabel *m_status;
    Probe *m_probe;
    QString m_chosen;
    bool m_supported = false;
};

class DestinationSelectPage : public QWizardPage {
    Q_OBJECT
public:
    explicit DestinationSelectPage(InstallerWizard *w);
    void initializePage() override;
    bool isComplete() const override;
    bool validatePage() override;
private:
    void update();
    InstallerWizard *m_wizard;
    QLineEdit *m_path;
    QLabel *m_space;
    QCheckBox *m_desktop;
    QCheckBox *m_menu;
    bool m_ok = false;
};

class InstallProgressPage : public QWizardPage {
    Q_OBJECT
public:
    explicit InstallProgressPage(InstallerWizard *w);
    void initializePage() override;
    bool isComplete() const override;
    void cancelInstall();
    bool busy() const;
private:
    InstallerWizard *m_wizard;
    QLabel *m_step;
    QProgressBar *m_bar;
    QPlainTextEdit *m_log;
    Installation *m_install;
    bool m_ok = false;
};

class FinishedPage : public QWizardPage {
    Q_OBJECT
public:
    explicit FinishedPage(InstallerWizard *w);
    void initializePage() override;
    bool validatePage() override;
private:
    InstallerWizard *m_wizard;
    QLabel *m_text;
    QCheckBox *m_launch;
};
